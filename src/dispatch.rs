use crate::config::CredentialStore;
use crate::navigation::{
    ChannelsOutput, SearchOutput, SearchRequest, SearchScope, TopicsOutput, encode_zulip_operand,
    list_channels, list_sites, list_topics, narrow_for_url, search,
};
use crate::url_parser;
use crate::zulip::{Message, ZulipClient};

fn format_messages(messages: &[Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        let dt = chrono::DateTime::from_timestamp(msg.timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| msg.timestamp.to_string());

        let topic_prefix = if !msg.subject.is_empty() {
            format!(" [{}]", msg.subject)
        } else {
            String::new()
        };

        out.push_str(&format!(
            "**{}** ({}){}\n{}\n\n",
            msg.sender_full_name, dt, topic_prefix, msg.content
        ));
    }
    out
}

fn parse_opt_u32(args: &[&str], key: &str) -> Option<u32> {
    let prefix = format!("{key}=");
    args.iter()
        .find(|a| a.starts_with(&prefix))
        .and_then(|a| a[prefix.len()..].parse().ok())
}

fn parse_format(value: &str) -> anyhow::Result<bool> {
    match value {
        "json" => Ok(true),
        "text" => Ok(false),
        _ => anyhow::bail!("Invalid format '{value}'; use text or json"),
    }
}

fn list_args<'a>(args: &'a [&'a str]) -> anyhow::Result<(Option<String>, bool)> {
    let mut words = Vec::new();
    let mut json = false;
    for arg in args {
        if let Some(value) = arg.strip_prefix("format=") {
            json = parse_format(value)?;
        } else {
            words.push(*arg);
        }
    }
    Ok(((!words.is_empty()).then(|| words.join(" ")), json))
}

fn search_args(url: &str, args: &[&str]) -> anyhow::Result<(SearchRequest, bool)> {
    let mut words = Vec::new();
    let mut scope = SearchScope::All;
    let mut limit = 50;
    let mut before = None;
    let mut json = false;
    for arg in args {
        if let Some(value) = arg.strip_prefix("scope=") {
            scope = value.parse()?;
        } else if let Some(value) = arg.strip_prefix("limit=") {
            limit = value.parse()?;
        } else if let Some(value) = arg.strip_prefix("before=") {
            before = Some(value.parse()?);
        } else if let Some(value) = arg.strip_prefix("format=") {
            json = parse_format(value)?;
        } else {
            words.push(*arg);
        }
    }
    Ok((
        SearchRequest {
            url: url.to_string(),
            query: words.join(" "),
            scope,
            limit,
            before,
        },
        json,
    ))
}

fn format_channels(data: &ChannelsOutput) -> String {
    if data.channels.is_empty() {
        return "No channels found.".to_string();
    }
    let mut out = format!("Channels ({}):\n\n", data.channels.len());
    for channel in &data.channels {
        out.push_str(&format!(
            "- **{}**{} — {}\n  {}\n",
            channel.name,
            if channel.is_private { " (private)" } else { "" },
            channel.description,
            channel.url
        ));
    }
    out
}

fn format_topics(data: &TopicsOutput) -> String {
    if data.topics.is_empty() {
        return format!("No topics found in {}.", data.channel_name);
    }
    let mut out = format!(
        "Topics in {} ({}):\n\n",
        data.channel_name,
        data.topics.len()
    );
    for topic in &data.topics {
        let name = if topic.name.is_empty() {
            "(empty topic)"
        } else {
            &topic.name
        };
        out.push_str(&format!("- **{}**\n  {}\n", name, topic.url));
    }
    out
}

fn format_search(data: &SearchOutput) -> String {
    if data.messages.is_empty() {
        return format!("No messages found for '{}'.", data.query);
    }
    let mut out = format!("Search results for '{}' (newest first):\n\n", data.query);
    for message in &data.messages {
        let dt = chrono::DateTime::from_timestamp(message.timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| message.timestamp.to_string());
        out.push_str(&format!(
            "**{}** ({}) — {}\n{}\n{}\n\n",
            message.sender, dt, message.location, message.content, message.url
        ));
    }
    if let Some(before) = data.next_before {
        out.push_str(&format!(
            "More results: repeat the search with before={before}"
        ));
    }
    out
}

pub const SETUP_HELP: &str = "\
Setup: configure Zulip credentials.

Step 1: Get your API key
  1. Log in to your Zulip instance in a browser
  2. Go to Personal settings > Account & privacy tab
  3. Click \"Manage your API key\" and enter your password

Step 2: Create ~/.zulookrc with your credentials:

  [[api]]
  site = \"https://your-org.zulipchat.com\"
  email = \"you@example.com\"
  key = \"your_api_key\"

  For multiple Zulip instances, add more [[api]] entries:

  [[api]]
  site = \"https://other-org.zulipchat.com\"
  email = \"you@other.com\"
  key = \"other_api_key\"

  zulook matches credentials to the URL you provide.

Alternatives:
  - Place a zuliprc file at ~/zuliprc (single instance only)
  - Set ZULIP_URL, ZULIP_EMAIL, ZULIP_API_KEY environment variables";

pub const HELP_TEXT: &str = "\
zulook — read-only access to Zulip conversations.

Commands:
  zulook <conversation_url> [limit=N]                   Fetch nearby messages (default: 100)
  zulook sites [format=json]                             List configured Zulip sites
  zulook channels <site_url> [name words] [format=json] List accessible channels
  zulook topics <channel_url> [name words] [format=json] List topics in a channel
  zulook search <site_or_conversation_url> <query words> [scope=all|channels|dms|public] [limit=N] [before=ID] [format=json]
                                                        Search messages (default limit: 50)
  zulook help                                           Show this help
  zulook setup                                          Show credential setup
  zulook mcp                                            Start MCP server
  zulook acp                                            Start ACP proxy

Site-wide search covers your personal message history, including subscribed channels and DMs.
Use scope=public to search accessible public channel history, including channels you have not joined.
Search a channel, topic, or DM URL to restrict results to that conversation.
Follow the result URLs to read nearby messages. Use before=ID to page to older matches.
Run `zulook search` for search-specific help.

Credentials: create ~/.zulookrc with your API keys (run `zulook setup` for details).

Examples:
  zulook channels https://rust-lang.zulipchat.com rust
  zulook topics https://rust-lang.zulipchat.com/#narrow/channel/213817-t-lang
  zulook search https://rust-lang.zulipchat.com async drop scope=public
  zulook search https://rust-lang.zulipchat.com/#narrow/channel/213817-t-lang borrow checker format=json";

pub const SEARCH_HELP: &str = "\
Search Zulip messages.

Usage:
  zulook search <site_or_conversation_url> <query words> [scope=all|channels|dms|public] [limit=N] [before=ID] [format=json]

The site or conversation URL is required first. Run `zulook sites` to list configured sites.
The query words are the text to search. scope=public is an option, not part of the query.
It searches public channel history and is valid with a site URL. With a conversation URL,
the search is restricted to that conversation, so omit scope.

Example:
  zulook search https://rust-lang.zulipchat.com async drop scope=public";

/// Dispatch a command and return the output as a string.
/// Used by both CLI mode and MCP tool mode.
pub async fn dispatch(store: &CredentialStore, command: &str) -> anyhow::Result<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let cmd = tokens.first().copied().unwrap_or("help");

    match cmd {
        "help" if tokens.get(1) == Some(&"search") => Ok(SEARCH_HELP.to_string()),

        "help" => Ok(HELP_TEXT.to_string()),

        "setup" => Ok(SETUP_HELP.to_string()),

        "sites" => {
            anyhow::ensure!(
                tokens.len() == 1 || (tokens.len() == 2 && tokens[1] == "format=json"),
                "Usage: zulook sites [format=json]"
            );
            let data = list_sites(store);
            if tokens.len() == 2 {
                Ok(serde_json::to_string_pretty(&data)?)
            } else if data.sites.is_empty() {
                Ok("No Zulip sites configured. Run `zulook setup`.".to_string())
            } else {
                Ok(data.sites.join("\n"))
            }
        }

        "channels" => {
            let site = tokens
                .get(1)
                .ok_or_else(|| anyhow::anyhow!("Usage: zulook channels <site_url> [name words]"))?;
            let (filter, json) = list_args(&tokens[2..])?;
            let data = list_channels(store, site, filter.as_deref()).await?;
            if json {
                Ok(serde_json::to_string_pretty(&data)?)
            } else {
                Ok(format_channels(&data))
            }
        }

        "topics" => {
            let channel = tokens.get(1).ok_or_else(|| {
                anyhow::anyhow!("Usage: zulook topics <channel_url> [name words]")
            })?;
            let (filter, json) = list_args(&tokens[2..])?;
            let data = list_topics(store, channel, filter.as_deref()).await?;
            if json {
                Ok(serde_json::to_string_pretty(&data)?)
            } else {
                Ok(format_topics(&data))
            }
        }

        "search" => {
            if tokens.len() == 1 || tokens.get(1) == Some(&"help") {
                return Ok(SEARCH_HELP.to_string());
            }
            let url = tokens[1];
            anyhow::ensure!(
                url.starts_with("https://") || url.starts_with("http://"),
                "The first search argument must be a Zulip site or conversation URL. \
                 `scope=public` is an option after the URL, not a query word.\n\n{SEARCH_HELP}"
            );
            let (request, json) = search_args(url, &tokens[2..])?;
            anyhow::ensure!(
                !request.query.trim().is_empty(),
                "Missing search query words.\n\n{SEARCH_HELP}"
            );
            let data = search(store, request).await?;
            if json {
                Ok(serde_json::to_string_pretty(&data)?)
            } else {
                Ok(format_search(&data))
            }
        }

        // URLs
        url if url.starts_with("https://") || url.starts_with("http://") => {
            let limit = parse_opt_u32(&tokens[1..], "limit").unwrap_or(100);
            handle_url(store, url, limit).await
        }

        _ => anyhow::bail!("Unknown command. Use 'help' to see usage."),
    }
}

async fn handle_url(store: &CredentialStore, url: &str, limit: u32) -> anyhow::Result<String> {
    let parsed = url_parser::parse_zulip_url(url)?;

    let creds = store.resolve(&parsed.base_url)?;
    let zulip = ZulipClient::new(creds.url, creds.email, creds.api_key);

    let narrow = narrow_for_url(&parsed)?;
    let label = if let Some(id) = parsed.with_message_id {
        format!("conversation containing message {id}")
    } else if parsed.is_dm() {
        let ids: Vec<String> = parsed.dm_user_ids.iter().map(|id| id.to_string()).collect();
        format!("DM with user(s) {}", ids.join(", "))
    } else if let Some(id) = parsed.stream_id {
        parsed
            .stream_name
            .clone()
            .unwrap_or_else(|| format!("channel #{}", id))
    } else if let Some(ref name) = parsed.stream_name {
        name.clone()
    } else if let Some(ref sender) = parsed.sender {
        format!("sender {}", sender)
    } else {
        anyhow::bail!("URL does not identify a conversation");
    };

    let (anchor, num_before, num_after) = if let Some(msg_id) = parsed.near_message_id {
        (msg_id.to_string(), limit / 2, limit / 2)
    } else {
        ("newest".to_string(), limit, 0)
    };

    let data = zulip
        .get_messages(narrow, &anchor, num_before, num_after, false, true)
        .await?;

    if data.messages.is_empty() {
        return Ok("No messages found at that URL.".to_string());
    }

    let topic_label = parsed
        .topic
        .as_ref()
        .map(|t| format!(" > {}", t))
        .unwrap_or_default();

    let mut messages = data.messages;
    messages.sort_by_key(|m| m.timestamp);

    let mut out = format!(
        "Messages from {}{}:\n\n{}",
        label,
        topic_label,
        format_messages(&messages)
    );

    // Build context URLs using first/last message IDs
    if let (Some(first), Some(last)) = (messages.first(), messages.last()) {
        let base = &parsed.base_url;
        let narrow_path = if let Some(id) = parsed.with_message_id {
            format!("with/{id}")
        } else if parsed.is_dm() {
            let ids: Vec<String> = parsed.dm_user_ids.iter().map(|id| id.to_string()).collect();
            format!("dm/{}", ids.join(","))
        } else if let Some(id) = parsed.stream_id {
            let mut p = format!("channel/{}", id);
            if let Some(ref topic) = parsed.topic {
                p.push_str(&format!("/topic/{}", encode_zulip_operand(topic)));
            }
            p
        } else if let Some(ref name) = parsed.stream_name {
            let mut p = format!("channel/{}", encode_zulip_operand(name));
            if let Some(ref topic) = parsed.topic {
                p.push_str(&format!("/topic/{}", encode_zulip_operand(topic)));
            }
            p
        } else if let Some(ref sender) = parsed.sender {
            format!("sender/{}", encode_zulip_operand(sender))
        } else {
            String::new()
        };

        if !narrow_path.is_empty() {
            out.push_str(&format!(
                "\nOlder: {}/#narrow/{}/near/{}",
                base, narrow_path, first.id
            ));
            out.push_str(&format!(
                "\nNewer: {}/#narrow/{}/near/{}",
                base, narrow_path, last.id
            ));
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_args() {
        let (request, json) = search_args(
            "https://example.com",
            &[
                "async",
                "drop",
                "scope=dms",
                "limit=20",
                "before=123",
                "format=json",
            ],
        )
        .unwrap();
        assert_eq!(request.query, "async drop");
        assert_eq!(request.scope, SearchScope::Dms);
        assert_eq!(request.limit, 20);
        assert_eq!(request.before, Some(123));
        assert!(json);
    }

    #[test]
    fn test_list_args() {
        let (filter, json) = list_args(&["language", "design", "format=json"]).unwrap();
        assert_eq!(filter.as_deref(), Some("language design"));
        assert!(json);
    }

    #[tokio::test]
    async fn test_search_help_explains_scope() {
        let store = CredentialStore::for_test("https://example.com".to_string());
        let help = dispatch(&store, "search").await.unwrap();
        assert!(help.contains("scope=public is an option, not part of the query"));
        assert_eq!(help, dispatch(&store, "help search").await.unwrap());

        let error = dispatch(&store, "search scope=public compiler")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("first search argument"));
        assert!(error.to_string().contains("site or conversation URL"));
    }
}
