/// Parse Zulip narrow URLs into their components.
///
/// Zulip URLs encode navigation state in the fragment:
///   https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test/near/12345
///   https://rust-lang.zulipchat.com/#narrow/dm/12345-User-Name
///
/// The fragment uses a custom encoding where `.` replaces `%` in percent-encoding.

#[derive(Debug, Clone)]
pub struct ZulipUrl {
    pub base_url: String,
    pub stream_id: Option<i64>,
    pub stream_name: Option<String>,
    pub topic: Option<String>,
    pub near_message_id: Option<i64>,
    pub with_message_id: Option<i64>,
    pub sender: Option<String>,
    /// DM user IDs (from dm/ or pm-with/ operator)
    pub dm_user_ids: Vec<i64>,
}

/// Decode Zulip's custom URL encoding: `.XX` → `%XX` → decoded char.
fn decode_zulip_operand(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'.'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            )
        {
            decoded.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Parse a stream operand like "213817-t-lang" into (Some(213817), "t-lang").
/// Also handles plain numeric "213817" and plain name "general".
fn parse_stream_operand(operand: &str) -> (Option<i64>, Option<String>) {
    if let Some(dash_pos) = operand.find('-') {
        let id_part = &operand[..dash_pos];
        let name_part = &operand[dash_pos + 1..];
        if let Ok(id) = id_part.parse::<i64>() {
            return (Some(id), Some(name_part.to_string()));
        }
    }
    if let Ok(id) = operand.parse::<i64>() {
        return (Some(id), None);
    }
    (None, Some(operand.to_string()))
}

/// Parse a DM operand like "12345-User-Name" or "12345,67890-Group".
/// Extracts user IDs (comma-separated before the first dash with a non-numeric part).
fn parse_dm_operand(operand: &str) -> Vec<i64> {
    // DM operands: "ID-name" for 1:1, "ID,ID,...-name" for group
    // Extract the numeric prefix before the human-readable hint
    let id_part = if let Some(dash_pos) = operand.find('-') {
        let before_dash = &operand[..dash_pos];
        // Check if everything before the dash is numeric/commas
        if before_dash.chars().all(|c| c.is_ascii_digit() || c == ',') {
            before_dash
        } else {
            operand
        }
    } else {
        operand
    };

    id_part
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect()
}

pub fn parse_zulip_url(url_str: &str) -> anyhow::Result<ZulipUrl> {
    let parsed = url::Url::parse(url_str)?;

    anyhow::ensure!(
        matches!(parsed.scheme(), "https" | "http") && parsed.host_str().is_some(),
        "Expected an HTTP(S) Zulip URL"
    );
    let base_url = parsed.origin().ascii_serialization();

    let fragment = parsed.fragment().unwrap_or("");

    let segments: Vec<&str> = fragment.split('/').filter(|s| !s.is_empty()).collect();

    if segments.first() != Some(&"narrow") {
        anyhow::bail!("Not a Zulip narrow URL (fragment doesn't start with #narrow/)");
    }

    let mut result = ZulipUrl {
        base_url,
        stream_id: None,
        stream_name: None,
        topic: None,
        near_message_id: None,
        with_message_id: None,
        sender: None,
        dm_user_ids: Vec::new(),
    };

    let mut i = 1;
    while i + 1 < segments.len() {
        let operator = segments[i];
        let operand = decode_zulip_operand(segments[i + 1]);
        i += 2;

        match operator {
            "stream" | "channel" => {
                let (id, name) = parse_stream_operand(&operand);
                result.stream_id = id;
                result.stream_name = name;
            }
            "topic" | "subject" => {
                result.topic = Some(operand);
            }
            "near" => {
                result.near_message_id = operand.parse().ok();
            }
            "with" => {
                result.with_message_id = operand.parse().ok();
            }
            "sender" | "from" => {
                result.sender = Some(operand);
            }
            "dm" | "pm-with" => {
                result.dm_user_ids = parse_dm_operand(&operand);
            }
            _ => {}
        }
    }

    Ok(result)
}

impl ZulipUrl {
    pub fn is_dm(&self) -> bool {
        !self.dm_user_ids.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_stream_topic() {
        let url = parse_zulip_url(
            "https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test",
        )
        .unwrap();
        assert_eq!(url.base_url, "https://rust-lang.zulipchat.com");
        assert_eq!(url.stream_id, Some(213817));
        assert_eq!(url.stream_name.as_deref(), Some("t-lang"));
        assert_eq!(url.topic.as_deref(), Some("test"));
        assert_eq!(url.near_message_id, None);
        assert!(!url.is_dm());
    }

    #[test]
    fn test_with_near() {
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/stream/42-general/topic/hello/near/99999",
        )
        .unwrap();
        assert_eq!(url.stream_id, Some(42));
        assert_eq!(url.topic.as_deref(), Some("hello"));
        assert_eq!(url.near_message_id, Some(99999));
    }

    #[test]
    fn test_channel_keyword() {
        let url =
            parse_zulip_url("https://example.zulipchat.com/#narrow/channel/100-design/topic/RFC")
                .unwrap();
        assert_eq!(url.stream_id, Some(100));
        assert_eq!(url.stream_name.as_deref(), Some("design"));
    }

    #[test]
    fn test_encoded_topic() {
        let url =
            parse_zulip_url("https://example.zulipchat.com/#narrow/stream/1-test/topic/my.2Etopic")
                .unwrap();
        assert_eq!(url.topic.as_deref(), Some("my.topic"));
    }

    #[test]
    fn test_unicode_topic() {
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/channel/1-test/topic/caf.C3.A9.20.26.20tea",
        )
        .unwrap();
        assert_eq!(url.topic.as_deref(), Some("café & tea"));
    }

    #[test]
    fn test_with_message_url() {
        let url = parse_zulip_url("https://example.zulipchat.com/#narrow/with/42/near/42").unwrap();
        assert_eq!(url.with_message_id, Some(42));
        assert_eq!(url.near_message_id, Some(42));
    }

    #[test]
    fn test_bare_channel_name_with_hyphen() {
        let url = parse_zulip_url("https://example.zulipchat.com/#narrow/channel/t-lang").unwrap();
        assert_eq!(url.stream_name.as_deref(), Some("t-lang"));
    }

    #[test]
    fn test_not_narrow_url() {
        let result = parse_zulip_url("https://example.zulipchat.com/#settings");
        assert!(result.is_err());
    }

    #[test]
    fn test_dm_url() {
        let url =
            parse_zulip_url("https://example.zulipchat.com/#narrow/dm/12345-User-Name").unwrap();
        assert!(url.is_dm());
        assert_eq!(url.dm_user_ids, vec![12345]);
        assert!(url.stream_id.is_none());
    }

    #[test]
    fn test_group_dm_url() {
        let url =
            parse_zulip_url("https://example.zulipchat.com/#narrow/dm/12345,67890-Group-Name")
                .unwrap();
        assert!(url.is_dm());
        assert_eq!(url.dm_user_ids, vec![12345, 67890]);
    }

    #[test]
    fn test_real_dm_url() {
        let url = parse_zulip_url(
            "https://rust-lang.zulipchat.com/#narrow/dm/116009,116107-dm/near/572348733",
        )
        .unwrap();
        assert!(url.is_dm());
        assert_eq!(url.dm_user_ids, vec![116009, 116107]);
        assert_eq!(url.near_message_id, Some(572348733));
    }

    #[test]
    fn test_pm_with_url() {
        let url =
            parse_zulip_url("https://example.zulipchat.com/#narrow/pm-with/42-Old-Style").unwrap();
        assert!(url.is_dm());
        assert_eq!(url.dm_user_ids, vec![42]);
    }
}
