use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::CredentialStore;
use crate::url_parser::{ZulipUrl, parse_zulip_url};
use crate::zulip::{DisplayRecipient, DmParticipant, Message, Narrow, ZulipClient};

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

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RecentScope {
    #[default]
    All,
    Channels,
    Dms,
}

impl std::str::FromStr for RecentScope {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> anyhow::Result<Self> {
        match value {
            "all" => Ok(Self::All),
            "channels" => Ok(Self::Channels),
            "dms" => Ok(Self::Dms),
            _ => anyhow::bail!("Invalid recent scope '{value}'; use all, channels, or dms"),
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

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecentHit {
    #[serde(flatten)]
    pub message: SearchHit,
    /// Personal Zulip message flags, including read, mentioned, and starred.
    pub flags: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecentOutput {
    pub messages: Vec<RecentHit>,
    /// Repeat with the same filters and this value as before to fetch older messages.
    pub next_before: Option<i64>,
}

pub struct RecentRequest {
    pub url: String,
    pub scope: RecentScope,
    pub unread_only: bool,
    pub mentions_only: bool,
    pub limit: u32,
    pub before: Option<i64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DmSummary {
    /// Other participants; for a conversation with yourself, this contains your user.
    pub participants: Vec<DmParticipant>,
    pub latest_message_id: i64,
    /// Conversation URL without an anchor, suitable for reading its newest messages.
    pub url: String,
}

pub const DM_HISTORY_NOTE: &str = "Zulip supplies a recent subset of DM conversations, not a complete history. Pagination only covers this subset. Use recent with scope=dms to scan older messages.";

#[derive(Debug, Serialize, JsonSchema)]
pub struct DmsOutput {
    pub current_user_id: i64,
    pub conversations: Vec<DmSummary>,
    /// Continue within the server's recent subset using the same filter.
    pub next_before: Option<i64>,
    pub note: String,
}

pub struct DmsRequest {
    pub site: String,
    pub filter: Option<String>,
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

fn validate_page(limit: u32, before: Option<i64>) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=1000).contains(&limit),
        "limit must be between 1 and 1000"
    );
    anyhow::ensure!(
        before.is_none_or(|id| id > 0),
        "before must be a positive message ID"
    );
    Ok(())
}

pub async fn recent(
    store: &CredentialStore,
    request: RecentRequest,
) -> anyhow::Result<RecentOutput> {
    validate_page(request.limit, request.before)?;
    let (base_url, conversation) = site_url(&request.url)?;
    let mut narrow = if let Some(parsed) = conversation {
        anyhow::ensure!(
            request.scope == RecentScope::All,
            "scope can only be used with a site URL"
        );
        narrow_for_url(&parsed)?
    } else {
        match request.scope {
            RecentScope::All => Vec::new(),
            RecentScope::Channels => vec![Narrow::is_dm(true)],
            RecentScope::Dms => vec![Narrow::is_dm(false)],
        }
    };
    if request.unread_only {
        narrow.push(Narrow::unread());
    }
    if request.mentions_only {
        narrow.push(Narrow::mentioned());
    }
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
    let mut messages: Vec<_> = data
        .messages
        .into_iter()
        .map(|message| {
            let flags = message.flags.clone();
            RecentHit {
                message: search_hit(&base_url, message),
                flags,
            }
        })
        .collect();
    messages.sort_by_key(|hit| std::cmp::Reverse(hit.message.id));
    // Honor the requested limit even if a server includes an extra anchor.
    let has_extra = messages.len() > request.limit as usize;
    messages.truncate(request.limit as usize);
    let next_before = if data.found_oldest && !has_extra {
        None
    } else {
        messages.last().map(|hit| hit.message.id)
    };
    Ok(RecentOutput {
        messages,
        next_before,
    })
}

pub async fn list_dms(store: &CredentialStore, request: DmsRequest) -> anyhow::Result<DmsOutput> {
    validate_page(request.limit, request.before)?;
    let (base_url, conversation) = site_url(&request.site)?;
    anyhow::ensure!(
        conversation.is_none(),
        "Expected a Zulip site URL for DM discovery"
    );
    let data = client_for(store, &base_url)?.get_dm_conversations().await?;
    let users: std::collections::HashMap<_, _> = data
        .realm_users
        .into_iter()
        .chain(data.realm_non_active_users)
        .chain(data.cross_realm_bots)
        .map(|user| (user.user_id, user.full_name))
        .collect();
    let filter = request.filter.as_deref().map(str::to_lowercase);
    let mut conversations: Vec<_> = data
        .recent_private_conversations
        .into_iter()
        .filter(|conversation| {
            request
                .before
                .is_none_or(|id| conversation.max_message_id < id)
        })
        .map(|conversation| {
            let mut ids = conversation.user_ids;
            if ids.is_empty() {
                ids.push(data.user_id);
            }
            ids.sort_unstable();
            ids.dedup();
            let participants: Vec<_> = ids
                .iter()
                .map(|&id| DmParticipant {
                    id,
                    full_name: users
                        .get(&id)
                        .cloned()
                        .unwrap_or_else(|| format!("user {id}")),
                })
                .collect();
            let ids = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
            DmSummary {
                participants,
                latest_message_id: conversation.max_message_id,
                url: format!("{base_url}/#narrow/dm/{ids}"),
            }
        })
        .filter(|conversation| {
            filter.as_ref().is_none_or(|query| {
                conversation
                    .participants
                    .iter()
                    .any(|user| user.full_name.to_lowercase().contains(query))
            })
        })
        .collect();
    conversations.sort_by_key(|conversation| std::cmp::Reverse(conversation.latest_message_id));
    let has_more = conversations.len() > request.limit as usize;
    conversations.truncate(request.limit as usize);
    let next_before = if has_more {
        conversations
            .last()
            .map(|conversation| conversation.latest_message_id)
    } else {
        None
    };
    Ok(DmsOutput {
        current_user_id: data.user_id,
        conversations,
        next_before,
        note: DM_HISTORY_NOTE.to_string(),
    })
}

fn search_hit(base_url: &str, message: Message) -> SearchHit {
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
        .map(|message| search_hit(&base_url, message))
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
        let (site, task) = mock_responses(vec![response]).await;
        (
            site,
            tokio::spawn(async move { task.await.unwrap().remove(0) }),
        )
    }

    async fn mock_responses(
        responses: Vec<serde_json::Value>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let site = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0, "request closed before completion");
                    request.extend_from_slice(&chunk[..count]);
                    if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]);
                        let content_length = headers
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .map_or(0, |(_, value)| value.trim().parse::<usize>().unwrap());
                        if request.len() >= end + 4 + content_length {
                            break;
                        }
                    }
                }
                let body = response.to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).await.unwrap();
                requests.push(String::from_utf8(request).unwrap());
            }
            requests
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

    fn recent_request(url: &str) -> RecentRequest {
        RecentRequest {
            url: url.to_string(),
            scope: RecentScope::All,
            unread_only: false,
            mentions_only: false,
            limit: 50,
            before: None,
        }
    }

    fn dms_request(site: &str) -> DmsRequest {
        DmsRequest {
            site: site.to_string(),
            filter: None,
            limit: 50,
            before: None,
        }
    }

    fn dm_snapshot() -> serde_json::Value {
        serde_json::json!({
            "result": "success",
            "queue_id": "temporary-test-queue",
            "user_id": 1,
            "recent_private_conversations": [
                {"user_ids": [2], "max_message_id": 20},
                {"user_ids": [], "max_message_id": 10},
                {"user_ids": [3, 2], "max_message_id": 30},
                {"user_ids": [4, 5], "max_message_id": 15}
            ],
            "realm_users": [
                {"user_id": 1, "full_name": "Me"},
                {"user_id": 2, "full_name": "Alice"}
            ],
            "realm_non_active_users": [{"user_id": 3, "full_name": "Bob"}],
            "cross_realm_bots": [{"user_id": 4, "full_name": "Reminder bot"}]
        })
    }

    fn assert_dm_snapshot_requests(site: &str, requests: &[String]) {
        assert_eq!(requests.len(), 2);
        assert!(requests[0].starts_with("POST /api/v1/register "));
        assert!(requests[1].starts_with("DELETE /api/v1/events "));
        for request in requests {
            assert!(request.to_lowercase().contains("authorization: basic "));
            assert_eq!(
                request_url(site, request).query(),
                None,
                "credentials and queue IDs should not be in the URL"
            );
        }
        let body = requests[0].split_once("\r\n\r\n").unwrap().1;
        let params: std::collections::HashMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();
        assert_eq!(params["event_types"], "[]");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&params["fetch_event_types"]).unwrap(),
            serde_json::json!(["recent_private_conversations", "realm_user"])
        );
        assert_eq!(
            requests[1].split_once("\r\n\r\n").unwrap().1,
            "queue_id=temporary-test-queue"
        );
    }

    #[tokio::test]
    async fn test_recent_filters_and_older_page() {
        let (site, task) = mock_responses(vec![
            serde_json::json!({
                "result": "success",
                "found_oldest": false,
                "messages": [
                    {"id": 10, "timestamp": 1000, "sender_full_name": "Alice", "content": "older", "display_recipient": [{"id": 2, "full_name": "Alice"}], "flags": ["mentioned"]},
                    {"id": 12, "timestamp": 900, "sender_full_name": "Bob", "content": "newer", "display_recipient": [{"id": 3, "full_name": "Bob"}], "flags": ["mentioned", "starred"]}
                ]
            }),
            serde_json::json!({
                "result": "success",
                "found_oldest": true,
                "messages": [
                    {"id": 8, "timestamp": 800, "sender_full_name": "Alice", "content": "oldest", "flags": ["mentioned"]}
                ]
            }),
        ])
        .await;
        let store = CredentialStore::for_test(site.clone());
        let mut request = recent_request(&site);
        request.scope = RecentScope::Dms;
        request.unread_only = true;
        request.mentions_only = true;
        request.limit = 2;
        let output = recent(&store, request).await.unwrap();
        assert_eq!(output.messages[0].message.id, 12);
        assert_eq!(output.messages[0].flags, ["mentioned", "starred"]);
        assert_eq!(output.messages[1].message.location, "DM: Alice");
        assert_eq!(output.next_before, Some(10));
        let json = serde_json::to_value(&output).unwrap();
        assert_eq!(json["messages"][0]["id"], 12);
        assert_eq!(json["messages"][0]["content"], "newer");
        assert_eq!(
            json["messages"][0]["url"],
            format!("{site}/#narrow/with/12/near/12")
        );

        let mut request = recent_request(&site);
        request.scope = RecentScope::Dms;
        request.unread_only = true;
        request.mentions_only = true;
        request.limit = 2;
        request.before = output.next_before;
        let older = recent(&store, request).await.unwrap();
        assert_eq!(older.messages[0].message.id, 8);
        assert_eq!(older.next_before, None);

        let requests = task.await.unwrap();
        for (index, request) in requests.iter().enumerate() {
            assert!(request.starts_with("GET /api/v1/messages?"));
            let params: std::collections::HashMap<_, _> = request_url(&site, request)
                .query_pairs()
                .into_owned()
                .collect();
            assert_eq!(params["num_before"], "2");
            assert_eq!(params["num_after"], "0");
            assert_eq!(params["apply_markdown"], "false");
            assert_eq!(params["anchor"], if index == 0 { "newest" } else { "10" });
            assert_eq!(
                params["include_anchor"],
                if index == 0 { "true" } else { "false" }
            );
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&params["narrow"]).unwrap(),
                serde_json::json!([
                    {"operator": "is", "operand": "dm"},
                    {"operator": "is", "operand": "unread"},
                    {"operator": "is", "operand": "mentioned"}
                ])
            );
        }
    }

    #[tokio::test]
    async fn test_recent_scopes_conversation_and_empty_results() {
        let empty = serde_json::json!({"result": "success", "found_oldest": true, "messages": []});
        let (site, task) = mock_responses(vec![empty.clone(), empty.clone(), empty]).await;
        let store = CredentialStore::for_test(site.clone());
        let output = recent(&store, recent_request(&site)).await.unwrap();
        assert!(output.messages.is_empty());
        assert_eq!(output.next_before, None);
        let mut request = recent_request(&site);
        request.scope = RecentScope::Channels;
        recent(&store, request).await.unwrap();
        // A near anchor in a conversation URL identifies context; recent always
        // starts with the newest messages unless an explicit before is supplied.
        let request = recent_request(&format!("{site}/#narrow/channel/42/topic/test/near/123"));
        recent(&store, request).await.unwrap();
        let requests = task.await.unwrap();
        let expected = [
            serde_json::json!([]),
            serde_json::json!([{"operator": "is", "operand": "dm", "negated": true}]),
            serde_json::json!([
                {"operator": "channel", "operand": 42},
                {"operator": "topic", "operand": "test"}
            ]),
        ];
        for (request, expected) in requests.iter().zip(expected) {
            let params: std::collections::HashMap<_, _> = request_url(&site, request)
                .query_pairs()
                .into_owned()
                .collect();
            assert_eq!(params["anchor"], "newest");
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&params["narrow"]).unwrap(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn test_recent_limit_keeps_cursor_for_extra_anchor() {
        let (site, task) = mock_response(serde_json::json!({
            "result": "success",
            "found_oldest": true,
            "messages": [
                {"id": 10, "timestamp": 10, "sender_full_name": "Alice", "content": "older"},
                {"id": 12, "timestamp": 12, "sender_full_name": "Alice", "content": "newer"}
            ]
        }))
        .await;
        let store = CredentialStore::for_test(site.clone());
        let mut request = recent_request(&site);
        request.limit = 1;
        let output = recent(&store, request).await.unwrap();
        assert_eq!(output.messages.len(), 1);
        assert_eq!(output.messages[0].message.id, 12);
        assert_eq!(output.next_before, Some(12));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn test_dms_participants_urls_order_and_queue_cleanup() {
        let (site, task) = mock_responses(vec![
            dm_snapshot(),
            serde_json::json!({"result": "success"}),
        ])
        .await;
        let store = CredentialStore::for_test(site.clone());
        let output = list_dms(&store, dms_request(&site)).await.unwrap();
        assert_eq!(output.current_user_id, 1);
        assert_eq!(output.conversations.len(), 4);
        assert_eq!(output.conversations[0].latest_message_id, 30);
        assert_eq!(output.conversations[0].participants[0].full_name, "Alice");
        assert_eq!(output.conversations[0].participants[1].full_name, "Bob");
        assert_eq!(
            output.conversations[0].url,
            format!("{site}/#narrow/dm/2,3")
        );
        assert_eq!(
            output.conversations[2].participants[0].full_name,
            "Reminder bot"
        );
        assert_eq!(output.conversations[2].participants[1].full_name, "user 5");
        assert_eq!(output.conversations[3].participants[0].full_name, "Me");
        assert_eq!(output.conversations[3].url, format!("{site}/#narrow/dm/1"));
        assert_eq!(output.next_before, None);
        assert!(output.note.contains("not a complete history"));
        for conversation in &output.conversations {
            let parsed = parse_zulip_url(&conversation.url).unwrap();
            assert!(parsed.is_dm());
            assert!(parsed.near_message_id.is_none());
        }
        assert_dm_snapshot_requests(&site, &task.await.unwrap());
    }

    #[tokio::test]
    async fn test_dms_cli_filter_json_and_paging() {
        let (site, task) = mock_responses(vec![
            dm_snapshot(),
            serde_json::json!({"result": "success"}),
            dm_snapshot(),
            serde_json::json!({"result": "success"}),
        ])
        .await;
        let store = CredentialStore::for_test(site.clone());
        let json =
            crate::dispatch::dispatch(&store, &format!("dms {site} ALICE limit=1 format=json"))
                .await
                .unwrap();
        let output: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(output["conversations"].as_array().unwrap().len(), 1);
        assert_eq!(output["conversations"][0]["latest_message_id"], 30);
        assert_eq!(output["next_before"], 30);
        let json = crate::dispatch::dispatch(
            &store,
            &format!("dms {site} ALICE limit=1 before=30 format=json"),
        )
        .await
        .unwrap();
        let output: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(output["conversations"][0]["latest_message_id"], 20);
        assert!(output["next_before"].is_null());
        assert_eq!(task.await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn test_dms_cleanup_before_decoding_invalid_snapshot() {
        let (site, task) = mock_responses(vec![
            serde_json::json!({"result": "success", "queue_id": "temporary-test-queue"}),
            serde_json::json!({"result": "success"}),
        ])
        .await;
        let store = CredentialStore::for_test(site.clone());
        let error = list_dms(&store, dms_request(&site)).await.unwrap_err();
        assert!(error.to_string().contains("missing field"));
        assert_dm_snapshot_requests(&site, &task.await.unwrap());
    }

    #[tokio::test]
    async fn test_scan_validation_before_network_access() {
        let site = "https://example.com";
        let store = CredentialStore::for_test(site.to_string());
        for (limit, before) in [(0, None), (1001, None), (50, Some(0)), (50, Some(-1))] {
            let mut request = recent_request(site);
            request.limit = limit;
            request.before = before;
            assert!(recent(&store, request).await.is_err());
            let mut request = dms_request(site);
            request.limit = limit;
            request.before = before;
            assert!(list_dms(&store, request).await.is_err());
        }
        let conversation = format!("{site}/#narrow/dm/2");
        let mut request = recent_request(&conversation);
        request.scope = RecentScope::Dms;
        assert!(
            recent(&store, request)
                .await
                .unwrap_err()
                .to_string()
                .contains("scope")
        );
        assert!(
            list_dms(&store, dms_request(&conversation))
                .await
                .unwrap_err()
                .to_string()
                .contains("site URL")
        );
    }
}
