use crate::url_parser;
use crate::zulip::{Message, Narrow, ZulipClient};

/// Encode a string for use in a Zulip URL fragment (reverse of url_parser::decode_zulip_operand).
fn encode_zulip_operand(s: &str) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!(".{:02X}", byte));
            }
        }
    }
    out
}

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

pub const SETUP_HELP: &str = "\
Setup: configure Zulip credentials.

How to get your zuliprc file:
  1. Log in to your Zulip instance in a browser
  2. Go to Personal settings > Account & privacy tab
  3. Click \"Manage your API key\" and enter your password
  4. Click \"Download .zuliprc\"
  5. Move the downloaded file to ~/zuliprc:
       mv ~/Downloads/zuliprc ~/zuliprc

  For a bot instead of your personal account:
  1. Go to Personal settings > Bots tab
  2. Click \"Add a new bot\" > choose \"Generic bot\"
  3. After creation, download the bot's zuliprc file
  4. Move to ~/zuliprc

Alternative: set environment variables instead:
  export ZULIP_URL=https://your-org.zulipchat.com
  export ZULIP_EMAIL=you@example.com
  export ZULIP_API_KEY=your_api_key";

pub const HELP_TEXT: &str = "\
zulook — read-only access to Zulip conversations.

Usage: zulook <url> [limit=N]
       zulook mcp                Start as MCP server (stdio transport)
       zulook acp                Start as ACP proxy (stdio transport)

Give a Zulip URL to fetch messages from that conversation.
The response includes Older/Newer URLs for paging through history.

Commands:
  <zulip_url> [limit=N]          Fetch messages at that URL (default limit: 100)
  help                           Show this help message
  setup                          Show how to configure Zulip credentials
  mcp                            Run as MCP server (stdio transport)
  acp                            Run as ACP proxy (stdio transport)

Credentials: place a zuliprc file at ~/zuliprc (run `zulook setup` for details).

Examples:
  zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test
  zulook https://rust-lang.zulipchat.com/#narrow/dm/116009,116107-dm/near/572348733
  zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test limit=50";

/// Dispatch a command and return the output as a string.
/// Used by both CLI mode and MCP tool mode.
pub async fn dispatch(zulip: &ZulipClient, command: &str) -> anyhow::Result<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let cmd = tokens.first().copied().unwrap_or("help");

    match cmd {
        "help" => Ok(HELP_TEXT.to_string()),

        "setup" => Ok(SETUP_HELP.to_string()),

        // URLs
        url if url.starts_with("https://") || url.starts_with("http://") => {
            let limit = parse_opt_u32(&tokens[1..], "limit").unwrap_or(100);
            handle_url(zulip, url, limit).await
        }

        _ => anyhow::bail!(
            "Expected a Zulip URL. Use 'help' to see usage.\n\
             Example: zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test"
        ),
    }
}

async fn handle_url(zulip: &ZulipClient, url: &str, limit: u32) -> anyhow::Result<String> {
    let parsed = url_parser::parse_zulip_url(url)?;

    let mut narrow = Vec::new();
    let label;

    if parsed.is_dm() {
        narrow.push(Narrow::dm(&parsed.dm_user_ids));
        let ids: Vec<String> = parsed.dm_user_ids.iter().map(|id| id.to_string()).collect();
        label = format!("DM with user(s) {}", ids.join(", "));
    } else if let Some(id) = parsed.stream_id {
        narrow.push(Narrow::channel_id(id));
        label = parsed
            .stream_name
            .clone()
            .unwrap_or_else(|| format!("stream #{}", id));
    } else if let Some(ref name) = parsed.stream_name {
        narrow.push(Narrow::channel(name));
        label = name.clone();
    } else {
        anyhow::bail!(
            "Could not extract stream or DM from URL. Expected #narrow/stream/..., #narrow/channel/..., or #narrow/dm/..."
        );
    }

    if let Some(ref topic) = parsed.topic {
        narrow.push(Narrow::topic(topic));
    }

    let (anchor, num_before, num_after) = if let Some(msg_id) = parsed.near_message_id {
        (msg_id.to_string(), limit / 2, limit / 2)
    } else {
        ("newest".to_string(), limit, 0)
    };

    let data = zulip
        .get_messages(narrow, &anchor, num_before, num_after, false)
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
        let narrow_path = if parsed.is_dm() {
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
