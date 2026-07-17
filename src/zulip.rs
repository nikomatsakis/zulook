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
}

#[derive(Debug, Deserialize)]
pub struct MessagesData {
    pub messages: Vec<Message>,
}

// --- Narrow filter construction ---

#[derive(Debug, Serialize)]
pub struct Narrow {
    pub operator: String,
    pub operand: serde_json::Value,
}

impl Narrow {
    pub fn channel(name: &str) -> Self {
        Self {
            operator: "channel".into(),
            operand: serde_json::Value::String(name.into()),
        }
    }

    pub fn channel_id(id: i64) -> Self {
        Self {
            operator: "channel".into(),
            operand: serde_json::json!(id),
        }
    }

    pub fn topic(name: &str) -> Self {
        Self {
            operator: "topic".into(),
            operand: serde_json::Value::String(name.into()),
        }
    }

    pub fn dm(user_ids: &[i64]) -> Self {
        let operand = if user_ids.len() == 1 {
            serde_json::json!(user_ids[0])
        } else {
            serde_json::json!(user_ids)
        };
        Self {
            operator: "dm".into(),
            operand,
        }
    }

    pub fn sender(sender_operand: &str) -> Self {
        if let Some(dash_pos) = sender_operand.find('-') {
            if let Ok(id) = sender_operand[..dash_pos].parse::<i64>() {
                return Self {
                    operator: "sender".into(),
                    operand: serde_json::json!(id),
                };
            }
        }
        if let Ok(id) = sender_operand.parse::<i64>() {
            return Self {
                operator: "sender".into(),
                operand: serde_json::json!(id),
            };
        }
        Self {
            operator: "sender".into(),
            operand: serde_json::Value::String(sender_operand.into()),
        }
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
                ],
            )
            .await?;
        Ok(resp.data)
    }
}
