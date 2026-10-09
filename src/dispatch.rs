use crate::config::CredentialStore;
use crate::url_parser;
use crate::zulip::{DisplayRecipient, Message, Narrow, ZulipClient};

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

/// Build the `channel/<id>-<name>` narrow segment the way Zulip's web app does
/// (spaces in the name become dashes before encoding).
fn channel_slug(id: i64, name: &str) -> String {
    format!("{}-{}", id, encode_zulip_operand(&name.replace(' ', "-")))
}

/// Permalink to a single message, if we have enough recipient info to build one.
fn message_permalink(base_url: &str, msg: &Message) -> Option<String> {
    match &msg.display_recipient {
        DisplayRecipient::Channel(name) => {
            let id = msg.stream_id?;
            Some(format!(
                "{}/#narrow/channel/{}/topic/{}/near/{}",
                base_url,
                channel_slug(id, name),
                encode_zulip_operand(&msg.subject),
                msg.id
            ))
        }
        DisplayRecipient::Users(users) => {
            let ids: Vec<String> = users.iter().map(|u| u.id.to_string()).collect();
            Some(format!(
                "{}/#narrow/dm/{}-dm/near/{}",
                base_url,
                ids.join(","),
                msg.id
            ))
        }
        DisplayRecipient::Unknown => None,
    }
}

#[derive(Default)]
struct FormatOptions<'a> {
    /// Show `#channel > topic` for each message (used when the narrow spans channels).
    show_channel: bool,
    /// Append a permalink to each message header. Requires `base_url`.
    links: bool,
    base_url: &'a str,
}

fn format_messages(messages: &[Message], opts: &FormatOptions) -> String {
    let mut out = String::new();
    for msg in messages {
        let dt = chrono::DateTime::from_timestamp(msg.timestamp, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| msg.timestamp.to_string());

        let context = match (&msg.display_recipient, opts.show_channel) {
            (DisplayRecipient::Channel(name), true) => {
                format!(" [#{} > {}]", name, msg.subject)
            }
            (DisplayRecipient::Users(users), true) => {
                let names: Vec<&str> = users.iter().map(|u| u.full_name.as_str()).collect();
                format!(" [DM: {}]", names.join(", "))
            }
            _ if !msg.subject.is_empty() => format!(" [{}]", msg.subject),
            _ => String::new(),
        };

        let link = if opts.links {
            message_permalink(opts.base_url, msg)
                .map(|l| format!(" <{}>", l))
                .unwrap_or_default()
        } else {
            String::new()
        };

        out.push_str(&format!(
            "**{}** ({}){}{}\n{}\n\n",
            msg.sender_full_name, dt, context, link, msg.content
        ));
    }
    out
}

/// True if a boolean flag is present: `name`, `--name`, or `name=true|1|yes`.
fn has_flag(args: &[&str], name: &str) -> bool {
    let dashed = format!("--{name}");
    let prefix = format!("{name}=");
    args.iter().any(|a| {
        *a == name
            || *a == dashed
            || a.strip_prefix(&prefix)
                .is_some_and(|v| matches!(v, "true" | "1" | "yes"))
    })
}

fn parse_opt_u32(args: &[&str], key: &str) -> Option<u32> {
    let prefix = format!("{key}=");
    args.iter()
        .find(|a| a.starts_with(&prefix))
        .and_then(|a| a[prefix.len()..].parse().ok())
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

Usage: zulook <url> [limit=N] [links]
       zulook mcp                Start as MCP server (stdio transport)
       zulook acp                Start as ACP proxy (stdio transport)

Give a Zulip URL to fetch messages from that conversation.
The response includes Older/Newer URLs for paging through history.
Supported narrows: channel/stream (+ topic), dm, and sender. Sender narrows
span channels, so each message is labeled with [#channel > topic].

Commands:
  <zulip_url> [limit=N] [links]  Fetch messages at that URL (default limit: 100)
                                 links: append a permalink to each message
                                 (also accepts --links or links=true)
  help                           Show this help message
  setup                          Show how to configure Zulip credentials
  mcp                            Run as MCP server (stdio transport)
  acp                            Run as ACP proxy (stdio transport)

Credentials: create ~/.zulookrc with your API keys (run `zulook setup` for details).

Examples:
  zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test
  zulook https://rust-lang.zulipchat.com/#narrow/dm/116009,116107-dm/near/572348733
  zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test limit=50
  zulook https://rust-lang.zulipchat.com/#narrow/sender/116883-tmandry limit=200 links";

/// Dispatch a command and return the output as a string.
/// Used by both CLI mode and MCP tool mode.
pub async fn dispatch(store: &CredentialStore, command: &str) -> anyhow::Result<String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    let cmd = tokens.first().copied().unwrap_or("help");

    match cmd {
        "help" => Ok(HELP_TEXT.to_string()),

        "setup" => Ok(SETUP_HELP.to_string()),

        // URLs
        url if url.starts_with("https://") || url.starts_with("http://") => {
            let limit = parse_opt_u32(&tokens[1..], "limit").unwrap_or(100);
            let links = has_flag(&tokens[1..], "links");
            handle_url(store, url, limit, links).await
        }

        _ => anyhow::bail!(
            "Expected a Zulip URL. Use 'help' to see usage.\n\
             Example: zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test"
        ),
    }
}

async fn handle_url(
    store: &CredentialStore,
    url: &str,
    limit: u32,
    links: bool,
) -> anyhow::Result<String> {
    let parsed = url_parser::parse_zulip_url(url)?;

    let creds = store.resolve(&parsed.base_url)?;
    let zulip = ZulipClient::new(creds.url, creds.email, creds.api_key);

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
    } else if let Some(ref sender) = parsed.sender {
        narrow.push(Narrow::sender(sender));
        label = format!("sender {}", sender);
    } else {
        anyhow::bail!(
            "Could not extract stream, DM, or sender from URL. Expected #narrow/stream/..., #narrow/channel/..., #narrow/dm/..., or #narrow/sender/..."
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

    let opts = FormatOptions {
        // Narrows not scoped to a single channel/DM span conversations, so label each message.
        show_channel: !parsed.is_dm() && parsed.stream_id.is_none() && parsed.stream_name.is_none(),
        links,
        base_url: &parsed.base_url,
    };

    let mut out = format!(
        "Messages from {}{}:\n\n{}",
        label,
        topic_label,
        format_messages(&messages, &opts)
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
    use crate::zulip::Recipient;

    fn stream_msg() -> Message {
        Message {
            id: 555,
            timestamp: 1_790_000_000,
            content: "hello".into(),
            sender_full_name: "Tyler Mandry".into(),
            subject: "Triage meeting 2026-09-23".into(),
            msg_type: "stream".into(),
            stream_id: Some(410673),
            display_recipient: DisplayRecipient::Channel("t-lang/meetings".into()),
        }
    }

    #[test]
    fn default_output_has_no_links() {
        let out = format_messages(&[stream_msg()], &FormatOptions::default());
        assert!(out.contains("[Triage meeting 2026-09-23]"));
        assert!(!out.contains("#narrow"));
        assert!(!out.contains("t-lang/meetings"));
    }

    #[test]
    fn show_channel_labels_channel_and_topic() {
        let opts = FormatOptions {
            show_channel: true,
            ..Default::default()
        };
        let out = format_messages(&[stream_msg()], &opts);
        assert!(out.contains("[#t-lang/meetings > Triage meeting 2026-09-23]"));
    }

    #[test]
    fn links_flag_appends_permalink() {
        let opts = FormatOptions {
            links: true,
            base_url: "https://rust-lang.zulipchat.com",
            ..Default::default()
        };
        let out = format_messages(&[stream_msg()], &opts);
        assert!(out.contains(
            "<https://rust-lang.zulipchat.com/#narrow/channel/410673-t-lang.2Fmeetings/topic/Triage.20meeting.202026-09-23/near/555>"
        ));
    }

    #[test]
    fn dm_permalink() {
        let msg = Message {
            msg_type: "private".into(),
            stream_id: None,
            subject: String::new(),
            display_recipient: DisplayRecipient::Users(vec![
                Recipient {
                    id: 1,
                    full_name: "A".into(),
                },
                Recipient {
                    id: 2,
                    full_name: "B".into(),
                },
            ]),
            ..stream_msg()
        };
        let p = message_permalink("https://x.zulipchat.com", &msg).unwrap();
        assert_eq!(p, "https://x.zulipchat.com/#narrow/dm/1,2-dm/near/555");
    }

    #[test]
    fn channel_slug_replaces_spaces() {
        assert_eq!(channel_slug(7, "project leads"), "7-project-leads");
    }

    #[test]
    fn flag_forms() {
        assert!(has_flag(&["links"], "links"));
        assert!(has_flag(&["limit=5", "--links"], "links"));
        assert!(has_flag(&["links=true"], "links"));
        assert!(!has_flag(&["links=false"], "links"));
        assert!(!has_flag(&["limit=5"], "links"));
    }
}
