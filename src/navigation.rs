use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::CredentialStore;
use crate::url_parser::{ZulipUrl, parse_zulip_url};
use crate::zulip::{DisplayRecipient, Narrow, ZulipClient};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchScope {
    #[default]
    All,
    Channels,
    Dms,
    Public,
}

impl std::str::FromStr for SearchScope {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "all" => Ok(Self::All),
            "channels" => Ok(Self::Channels),
            "dms" => Ok(Self::Dms),
            "public" => Ok(Self::Public),
            _ => anyhow::bail!("Invalid scope '{value}'; use all, channels, dms, or public"),
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ChannelSummary {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub is_private: bool,
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ChannelsOutput {
    pub channels: Vec<ChannelSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SitesOutput {
    pub sites: Vec<String>,
}

pub fn list_sites(store: &CredentialStore) -> SitesOutput {
    SitesOutput {
        sites: store.sites(),
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopicSummary {
    pub name: String,
    pub latest_message_id: i64,
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct TopicsOutput {
    pub channel_id: i64,
    pub channel_name: String,
    pub topics: Vec<TopicSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchHit {
    pub id: i64,
    pub timestamp: i64,
    pub sender: String,
    pub location: String,
    pub content: String,
    pub url: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SearchOutput {
    pub query: String,
    pub messages: Vec<SearchHit>,
    /// Pass this as `before` on the next search to continue toward older matches.
    pub next_before: Option<i64>,
}

pub struct SearchRequest {
    pub url: String,
    pub query: String,
    pub scope: SearchScope,
    pub limit: u32,
    pub before: Option<i64>,
}

pub fn encode_zulip_operand(s: &str) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => out.push(byte as char),
            _ => out.push_str(&format!(".{byte:02X}")),
        }
    }
    out
}

fn site_url(url: &str) -> anyhow::Result<(String, Option<ZulipUrl>)> {
    let parsed = url::Url::parse(url)?;
    anyhow::ensure!(
        matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some(),
        "Expected an HTTP(S) Zulip URL"
    );
    let base_url = parsed.origin().ascii_serialization();
    let narrow = match parsed.fragment() {
        Some(fragment) if fragment.starts_with("narrow/") => Some(parse_zulip_url(url)?),
        Some(fragment) if !fragment.is_empty() => {
            anyhow::bail!("Expected a Zulip conversation URL or site URL")
        }
        _ => None,
    };
    Ok((base_url, narrow))
}

fn client_for(store: &CredentialStore, base_url: &str) -> anyhow::Result<ZulipClient> {
    let creds = store.resolve(base_url)?;
    Ok(ZulipClient::new(creds.url, creds.email, creds.api_key))
}

pub fn narrow_for_url(parsed: &ZulipUrl) -> anyhow::Result<Vec<Narrow>> {
    let mut narrow = Vec::new();
    if let Some(id) = parsed.with_message_id {
        narrow.push(Narrow::with_message(id));
    } else if parsed.is_dm() {
        narrow.push(Narrow::dm(&parsed.dm_user_ids));
    } else if let Some(id) = parsed.stream_id {
        narrow.push(Narrow::channel_id(id));
    } else if let Some(name) = &parsed.stream_name {
        narrow.push(Narrow::channel(name));
    } else if let Some(sender) = &parsed.sender {
        narrow.push(Narrow::sender(sender));
    } else {
        anyhow::bail!("URL does not identify a channel, topic, DM, sender, or message");
    }
    if let Some(topic) = &parsed.topic {
        narrow.push(Narrow::topic(topic));
    }
    Ok(narrow)
}

pub async fn list_channels(
    store: &CredentialStore,
    site: &str,
    filter: Option<&str>,
) -> anyhow::Result<ChannelsOutput> {
    let (base_url, _) = site_url(site)?;
    let data = client_for(store, &base_url)?.get_streams().await?;
    let filter = filter.map(str::to_lowercase);
    let mut channels: Vec<_> = data
        .streams
        .into_iter()
        .filter(|stream| {
            filter.as_ref().is_none_or(|query| {
                stream.name.to_lowercase().contains(query)
                    || stream.description.to_lowercase().contains(query)
            })
        })
        .map(|stream| ChannelSummary {
            id: stream.stream_id,
            name: stream.name,
            description: stream.description,
            is_private: stream.invite_only,
            url: format!("{base_url}/#narrow/channel/{}", stream.stream_id),
        })
        .collect();
    channels.sort_by_key(|channel| channel.name.to_lowercase());
    Ok(ChannelsOutput { channels })
}

pub async fn list_topics(
    store: &CredentialStore,
    channel_url: &str,
    filter: Option<&str>,
) -> anyhow::Result<TopicsOutput> {
    let (base_url, parsed) = site_url(channel_url)?;
    let parsed = parsed.ok_or_else(|| anyhow::anyhow!("Expected a Zulip channel URL"))?;
    anyhow::ensure!(
        !parsed.is_dm() && parsed.with_message_id.is_none() && parsed.sender.is_none(),
        "Expected a Zulip channel URL"
    );

    let zulip = client_for(store, &base_url)?;
    let (channel_id, channel_name) = if let Some(id) = parsed.stream_id {
        (
            id,
            parsed
                .stream_name
                .unwrap_or_else(|| format!("channel {id}")),
        )
    } else if let Some(name) = parsed.stream_name {
        let streams = zulip.get_streams().await?;
        let stream = streams
            .streams
            .into_iter()
            .find(|stream| stream.name.eq_ignore_ascii_case(&name))
            .ok_or_else(|| anyhow::anyhow!("Channel '{name}' not found"))?;
        (stream.stream_id, stream.name)
    } else {
        anyhow::bail!("Expected a Zulip channel URL");
    };

    let data = zulip.get_stream_topics(channel_id).await?;
    let filter = filter.map(str::to_lowercase);
    let topics = data
        .topics
        .into_iter()
        .filter(|topic| {
            filter
                .as_ref()
                .is_none_or(|query| topic.name.to_lowercase().contains(query))
        })
        .map(|topic| {
            let url = if topic.name.is_empty() {
                format!("{base_url}/#narrow/with/{0}/near/{0}", topic.max_id)
            } else {
                format!(
                    "{base_url}/#narrow/channel/{channel_id}/topic/{}/near/{}",
                    encode_zulip_operand(&topic.name),
                    topic.max_id
                )
            };
            TopicSummary {
                url,
                name: topic.name,
                latest_message_id: topic.max_id,
            }
        })
        .collect();
    Ok(TopicsOutput {
        channel_id,
        channel_name,
        topics,
    })
}

pub async fn search(
    store: &CredentialStore,
    request: SearchRequest,
) -> anyhow::Result<SearchOutput> {
    let query = request.query.trim();
    anyhow::ensure!(!query.is_empty(), "Search query cannot be empty");
    anyhow::ensure!(
        (1..=1000).contains(&request.limit),
        "Search limit must be between 1 and 1000"
    );
    anyhow::ensure!(
        request.before.is_none_or(|id| id > 0),
        "before must be a positive message ID"
    );

    let (base_url, conversation) = site_url(&request.url)?;
    let mut narrow = if let Some(parsed) = conversation {
        anyhow::ensure!(
            request.scope == SearchScope::All,
            "scope can only be used with a site URL"
        );
        narrow_for_url(&parsed)?
    } else {
        match request.scope {
            SearchScope::All => Vec::new(),
            SearchScope::Channels => vec![Narrow::is_dm(true)],
            SearchScope::Dms => vec![Narrow::is_dm(false)],
            SearchScope::Public => vec![Narrow::public_channels()],
        }
    };
    narrow.push(Narrow::search(query));

    let anchor = request
        .before
        .map_or_else(|| "newest".to_string(), |id| id.to_string());
    let data = client_for(store, &base_url)?
        .get_messages(
            narrow,
            &anchor,
            request.limit,
            0,
            false,
            request.before.is_none(),
        )
        .await?;
    let next_before = if data.found_oldest {
        None
    } else {
        data.messages.first().map(|message| message.id)
    };
    let mut messages: Vec<_> = data
        .messages
        .into_iter()
        .map(|message| {
            let location = match &message.display_recipient {
                Some(DisplayRecipient::Channel(channel)) => {
                    if message.subject.is_empty() {
                        channel.clone()
                    } else {
                        format!("{channel} > {}", message.subject)
                    }
                }
                Some(DisplayRecipient::Dm(participants)) => format!(
                    "DM: {}",
                    participants
                        .iter()
                        .map(|user| user.full_name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                None => "Unknown conversation".to_string(),
            };
            SearchHit {
                id: message.id,
                timestamp: message.timestamp,
                sender: message.sender_full_name,
                location,
                content: message.content,
                url: format!("{base_url}/#narrow/with/{0}/near/{0}", message.id),
            }
        })
        .collect();
    messages.sort_by_key(|message| std::cmp::Reverse(message.id));
    Ok(SearchOutput {
        query: query.to_string(),
        messages,
        next_before,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn mock_response(
        response: serde_json::Value,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0, "request closed before headers");
                request.extend_from_slice(&chunk[..count]);
                if request.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let body = response.to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        (site, task)
    }

    fn request_url(site: &str, request: &str) -> url::Url {
        let path = request
            .lines()
            .next()
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap();
        url::Url::parse(&format!("{site}{path}")).unwrap()
    }

    #[test]
    fn test_topic_operand_encoding() {
        assert_eq!(encode_zulip_operand("café & tea"), "caf.C3.A9.20.26.20tea");
    }

    #[test]
    fn test_scope_parsing() {
        assert_eq!("dms".parse::<SearchScope>().unwrap(), SearchScope::Dms);
        assert!("dm".parse::<SearchScope>().is_err());
    }

    #[test]
    fn test_with_message_narrow() {
        let parsed = parse_zulip_url("https://example.com/#narrow/with/42/near/42").unwrap();
        let narrow = narrow_for_url(&parsed).unwrap();
        assert_eq!(
            serde_json::to_value(narrow).unwrap(),
            serde_json::json!([{"operator": "with", "operand": 42}])
        );
    }

    #[tokio::test]
    async fn test_list_channels_wire_response() {
        let (site, task) = mock_response(serde_json::json!({
            "result": "success",
            "streams": [
                {"stream_id": 42, "name": "compiler", "description": "Rust compiler", "invite_only": false},
                {"stream_id": 43, "name": "private", "description": "Internal", "invite_only": true}
            ]
        }))
        .await;
        let store = CredentialStore::for_test(site.clone());
        let output = list_channels(&store, &site, Some("Compiler"))
            .await
            .unwrap();
        assert_eq!(output.channels.len(), 1);
        assert_eq!(output.channels[0].id, 42);
        assert_eq!(output.channels[0].url, format!("{site}/#narrow/channel/42"));
        let request = task.await.unwrap();
        assert_eq!(request_url(&site, &request).path(), "/api/v1/streams");
    }

    #[tokio::test]
    async fn test_list_topics_wire_response() {
        let (site, task) = mock_response(serde_json::json!({
            "result": "success",
            "topics": [
                {"name": "café & tea", "max_id": 123},
                {"name": "", "max_id": 100}
            ]
        }))
        .await;
        let store = CredentialStore::for_test(site.clone());
        let output = list_topics(&store, &format!("{site}/#narrow/channel/42-t-lang"), None)
            .await
            .unwrap();
        assert_eq!(output.topics.len(), 2);
        assert!(
            output.topics[0]
                .url
                .contains("/topic/caf.C3.A9.20.26.20tea/")
        );
        assert!(output.topics[1].url.contains("/#narrow/with/100/near/100"));
        let request = task.await.unwrap();
        let url = request_url(&site, &request);
        assert_eq!(url.path(), "/api/v1/users/me/42/topics");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "allow_empty_topic_name")
                .unwrap()
                .1,
            "true"
        );
    }

    #[tokio::test]
    async fn test_search_wire_response_and_paging() {
        let (site, task) = mock_response(serde_json::json!({
            "result": "success",
            "found_oldest": false,
            "messages": [
                {"id": 10, "timestamp": 1000, "sender_full_name": "Alice", "content": "older", "subject": "topic", "stream_id": 42, "display_recipient": "compiler"},
                {"id": 12, "timestamp": 1200, "sender_full_name": "Bob", "content": "newer", "subject": "", "display_recipient": [{"id": 1, "full_name": "Alice"}, {"id": 2, "full_name": "Bob"}]}
            ]
        }))
        .await;
        let store = CredentialStore::for_test(site.clone());
        let output = search(
            &store,
            SearchRequest {
                url: site.clone(),
                query: "borrow checker".to_string(),
                scope: SearchScope::Dms,
                limit: 20,
                before: Some(15),
            },
        )
        .await
        .unwrap();
        assert_eq!(output.messages.len(), 2);
        assert_eq!(output.messages[0].id, 12);
        assert_eq!(output.messages[1].location, "compiler > topic");
        assert_eq!(output.next_before, Some(10));
        assert_eq!(
            output.messages[0].url,
            format!("{site}/#narrow/with/12/near/12")
        );
        let request = task.await.unwrap();
        let url = request_url(&site, &request);
        assert_eq!(url.path(), "/api/v1/messages");
        let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(params["anchor"], "15");
        assert_eq!(params["include_anchor"], "false");
        assert_eq!(params["num_before"], "20");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&params["narrow"]).unwrap(),
            serde_json::json!([
                {"operator": "is", "operand": "dm"},
                {"operator": "search", "operand": "borrow checker"}
            ])
        );
    }
}
