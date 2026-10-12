use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ZulipClient {
    client: Client,
    base_url: String,
    email: String,
    api_key: String,
}

// --- API response types ---

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    #[serde(flatten)]
    data: T,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Message {
    pub id: i64,
    pub timestamp: i64,
    pub content: String,
    pub sender_full_name: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub stream_id: Option<i64>,
    #[serde(default)]
    pub display_recipient: Option<DisplayRecipient>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(untagged)]
pub enum DisplayRecipient {
    Channel(String),
    Dm(Vec<DmParticipant>),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DmParticipant {
    pub id: i64,
    pub full_name: String,
}

#[derive(Debug, Deserialize)]
pub struct MessagesData {
    pub messages: Vec<Message>,
    #[serde(default)]
    pub found_oldest: bool,
}

#[derive(Debug, Deserialize)]
pub struct StreamsData {
    pub streams: Vec<Stream>,
}

#[derive(Debug, Deserialize)]
pub struct Stream {
    pub stream_id: i64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub invite_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct TopicsData {
    pub topics: Vec<Topic>,
}

#[derive(Debug, Deserialize)]
pub struct Topic {
    pub name: String,
    pub max_id: i64,
}

// --- Narrow filter construction ---

#[derive(Debug, Serialize)]
pub struct Narrow {
    pub operator: String,
    pub operand: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub negated: Option<bool>,
}

impl Narrow {
    fn new(operator: &str, operand: serde_json::Value) -> Self {
        Self {
            operator: operator.into(),
            operand,
            negated: None,
        }
    }

    pub fn channel(name: &str) -> Self {
        Self::new("channel", serde_json::json!(name))
    }

    pub fn channel_id(id: i64) -> Self {
        Self::new("channel", serde_json::json!(id))
    }

    pub fn topic(name: &str) -> Self {
        Self::new("topic", serde_json::json!(name))
    }

    pub fn dm(user_ids: &[i64]) -> Self {
        let operand = if user_ids.len() == 1 {
            serde_json::json!(user_ids[0])
        } else {
            serde_json::json!(user_ids)
        };
        Self::new("dm", operand)
    }

    pub fn with_message(id: i64) -> Self {
        Self::new("with", serde_json::json!(id))
    }

    pub fn search(query: &str) -> Self {
        Self::new("search", serde_json::json!(query))
    }

    pub fn is_dm(negated: bool) -> Self {
        let mut narrow = Self::new("is", serde_json::json!("dm"));
        if negated {
            narrow.negated = Some(true);
        }
        narrow
    }

    pub fn public_channels() -> Self {
        Self::new("channels", serde_json::json!("public"))
    }

    pub fn sender(sender_operand: &str) -> Self {
        if let Some(dash_pos) = sender_operand.find('-')
            && let Ok(id) = sender_operand[..dash_pos].parse::<i64>()
        {
            return Self::new("sender", serde_json::json!(id));
        }
        if let Ok(id) = sender_operand.parse::<i64>() {
            return Self::new("sender", serde_json::json!(id));
        }
        Self::new("sender", serde_json::json!(sender_operand))
    }
}

// --- Client implementation ---

impl ZulipClient {
    pub fn new(base_url: String, email: String, api_key: String) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            email,
            api_key,
        }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> anyhow::Result<T> {
        let resp = self
            .client
            .get(self.api_url(path))
            .basic_auth(&self.email, Some(&self.api_key))
            .query(query)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Zulip API error (HTTP {}): {}", status, body);
        }

        Ok(resp.json().await?)
    }

    pub async fn get_messages(
        &self,
        narrow: Vec<Narrow>,
        anchor: &str,
        num_before: u32,
        num_after: u32,
        apply_markdown: bool,
        include_anchor: bool,
    ) -> anyhow::Result<MessagesData> {
        let narrow_json = serde_json::to_string(&narrow)?;
        let resp: ApiResponse<MessagesData> = self
            .get(
                "/messages",
                &[
                    ("narrow", narrow_json),
                    ("anchor", anchor.to_string()),
                    ("num_before", num_before.to_string()),
                    ("num_after", num_after.to_string()),
                    ("apply_markdown", apply_markdown.to_string()),
                    ("include_anchor", include_anchor.to_string()),
                    ("allow_empty_topic_name", "true".to_string()),
                ],
            )
            .await?;
        Ok(resp.data)
    }

    pub async fn get_streams(&self) -> anyhow::Result<StreamsData> {
        let resp: ApiResponse<StreamsData> = self.get("/streams", &[]).await?;
        Ok(resp.data)
    }

    pub async fn get_stream_topics(&self, stream_id: i64) -> anyhow::Result<TopicsData> {
        let resp: ApiResponse<TopicsData> = self
            .get(
                &format!("/users/me/{stream_id}/topics"),
                &[("allow_empty_topic_name", "true".to_string())],
            )
            .await?;
        Ok(resp.data)
    }
}
