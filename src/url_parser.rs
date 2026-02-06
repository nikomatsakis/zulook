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
    /// DM user IDs (from dm/ or pm-with/ operator)
    pub dm_user_ids: Vec<i64>,
}

/// Decode Zulip's custom URL encoding: `.XX` → `%XX` → decoded char.
fn decode_zulip_operand(encoded: &str) -> String {
    let mut result = String::new();
    let mut chars = encoded.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '.' {
            let mut hex = String::new();
            if let Some(&h1) = chars.peek() {
                hex.push(h1);
                chars.next();
            }
            if let Some(&h2) = chars.peek() {
                hex.push(h2);
                chars.next();
            }
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    result.push(byte as char);
                } else {
                    result.push('.');
                    result.push_str(&hex);
                }
            } else {
                result.push('.');
                result.push_str(&hex);
            }
        } else {
            result.push(ch);
        }
    }
    result
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
    (None, Some(operand.replace('-', " ")))
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

    let base_url = format!(
        "{}://{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or("")
    );

    let fragment = parsed.fragment().unwrap_or("");

    let segments: Vec<&str> = fragment
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if segments.first() != Some(&"narrow") {
        anyhow::bail!("Not a Zulip narrow URL (fragment doesn't start with #narrow/)");
    }

    let mut result = ZulipUrl {
        base_url,
        stream_id: None,
        stream_name: None,
        topic: None,
        near_message_id: None,
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
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/channel/100-design/topic/RFC",
        )
        .unwrap();
        assert_eq!(url.stream_id, Some(100));
        assert_eq!(url.stream_name.as_deref(), Some("design"));
    }

    #[test]
    fn test_encoded_topic() {
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/stream/1-test/topic/my.2Etopic",
        )
        .unwrap();
        assert_eq!(url.topic.as_deref(), Some("my.topic"));
    }

    #[test]
    fn test_not_narrow_url() {
        let result = parse_zulip_url("https://example.zulipchat.com/#settings");
        assert!(result.is_err());
    }

    #[test]
    fn test_dm_url() {
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/dm/12345-User-Name",
        )
        .unwrap();
        assert!(url.is_dm());
        assert_eq!(url.dm_user_ids, vec![12345]);
        assert!(url.stream_id.is_none());
    }

    #[test]
    fn test_group_dm_url() {
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/dm/12345,67890-Group-Name",
        )
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
        let url = parse_zulip_url(
            "https://example.zulipchat.com/#narrow/pm-with/42-Old-Style",
        )
        .unwrap();
        assert!(url.is_dm());
        assert_eq!(url.dm_user_ids, vec![42]);
    }
}
