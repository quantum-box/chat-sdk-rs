use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::adapter::ChatAdapter;
use crate::error::{ChatError, ChatResult};
use crate::model::{Channel, Message, MessageId, Reaction, SendMessage, Thread, User};

const BASE_URL: &str = "https://slack.com/api";

/// Slack adapter implementing [`ChatAdapter`].
pub struct SlackAdapter {
    client: Client,
    token: String,
}

impl SlackAdapter {
    /// Create a new Slack adapter with the given bot token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            token: token.into(),
        }
    }

    /// Create a new Slack adapter with a custom [`reqwest::Client`].
    pub fn with_client(client: Client, token: impl Into<String>) -> Self {
        Self {
            client,
            token: token.into(),
        }
    }

    async fn api_get<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        query: &[(&str, &str)],
    ) -> ChatResult<T> {
        let resp = self
            .client
            .get(format!("{BASE_URL}/{method}"))
            .bearer_auth(&self.token)
            .query(query)
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        check_slack_error(&body)?;
        serde_json::from_value(body).map_err(ChatError::Serialization)
    }

    async fn api_post<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        body: &serde_json::Value,
    ) -> ChatResult<T> {
        let resp = self
            .client
            .post(format!("{BASE_URL}/{method}"))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        check_slack_error(&body)?;
        serde_json::from_value(body).map_err(ChatError::Serialization)
    }
}

/// Map Slack API error codes to typed [`ChatError`] variants.
fn check_slack_error(body: &serde_json::Value) -> ChatResult<()> {
    if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(());
    }
    let code = body
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown_error");
    match code {
        "not_authed" | "invalid_auth" | "token_revoked" | "account_inactive" => {
            Err(ChatError::Auth(code.to_string()))
        }
        "channel_not_found" => Err(ChatError::ChannelNotFound(code.to_string())),
        "ratelimited" => {
            let retry = body
                .get("headers")
                .and_then(|h| h.get("Retry-After"))
                .and_then(|v| v.as_u64())
                .unwrap_or(30);
            Err(ChatError::RateLimited {
                retry_after_secs: retry,
            })
        }
        _ => Err(ChatError::Api(format!("slack: {code}"))),
    }
}

// ── Slack API response types ────────────────────────────────────────

#[derive(Deserialize)]
struct SlackPostMessageResp {
    ts: String,
}

#[derive(Deserialize)]
struct SlackHistoryResp {
    messages: Vec<SlackMessage>,
}

#[derive(Deserialize)]
struct SlackMessage {
    ts: String,
    user: Option<String>,
    text: Option<String>,
    thread_ts: Option<String>,
    reply_count: Option<u32>,
}

#[derive(Deserialize)]
struct SlackChannelListResp {
    channels: Vec<SlackChannel>,
}

#[derive(Deserialize)]
struct SlackChannel {
    id: String,
    name: String,
    topic: Option<SlackTopic>,
}

#[derive(Deserialize)]
struct SlackTopic {
    value: String,
}

#[derive(Deserialize)]
struct SlackReactionsResp {
    message: SlackReactionsMessage,
}

#[derive(Deserialize)]
struct SlackReactionsMessage {
    reactions: Option<Vec<SlackReaction>>,
}

#[derive(Deserialize)]
struct SlackReaction {
    name: String,
    users: Vec<String>,
}

#[derive(Deserialize)]
struct SlackRepliesResp {
    messages: Vec<SlackMessage>,
}

// ── Helpers ─────────────────────────────────────────────────────────

fn slack_msg_to_message(channel: &str, m: &SlackMessage) -> Message {
    Message {
        id: MessageId(m.ts.clone()),
        channel: channel.to_string(),
        author: User {
            id: m.user.clone().unwrap_or_default(),
            name: m.user.clone().unwrap_or_default(),
            display_name: None,
        },
        text: m.text.clone().unwrap_or_default(),
        thread: m.thread_ts.as_ref().map(|ts| Thread {
            parent_id: MessageId(ts.clone()),
            reply_count: m.reply_count.unwrap_or(0),
        }),
        timestamp: m.ts.clone(),
    }
}

// ── ChatAdapter impl ────────────────────────────────────────────────

#[async_trait]
impl ChatAdapter for SlackAdapter {
    fn platform(&self) -> &str {
        "slack"
    }

    async fn send_message(&self, msg: SendMessage) -> ChatResult<MessageId> {
        let mut body = serde_json::json!({
            "channel": msg.channel,
            "text": msg.text,
        });
        if let Some(ref thread_id) = msg.thread_id {
            body["thread_ts"] = serde_json::Value::String(thread_id.0.clone());
        }
        if !msg.cards.is_empty() {
            let attachments: Vec<serde_json::Value> =
                msg.cards.iter().map(|c| c.to_slack_blocks()).collect();
            body["attachments"] = serde_json::Value::Array(attachments);
        }
        let resp: SlackPostMessageResp = self.api_post("chat.postMessage", &body).await?;
        Ok(MessageId(resp.ts))
    }

    async fn get_messages(&self, channel: &str, limit: usize) -> ChatResult<Vec<Message>> {
        let limit_str = limit.to_string();
        let resp: SlackHistoryResp = self
            .api_get(
                "conversations.history",
                &[("channel", channel), ("limit", &limit_str)],
            )
            .await?;
        Ok(resp
            .messages
            .iter()
            .map(|m| slack_msg_to_message(channel, m))
            .collect())
    }

    async fn list_channels(&self) -> ChatResult<Vec<Channel>> {
        let resp: SlackChannelListResp = self
            .api_get(
                "conversations.list",
                &[
                    ("types", "public_channel,private_channel"),
                    ("limit", "200"),
                ],
            )
            .await?;
        Ok(resp
            .channels
            .into_iter()
            .map(|c| Channel {
                id: c.id,
                name: c.name,
                topic: c.topic.map(|t| t.value),
            })
            .collect())
    }

    async fn add_reaction(
        &self,
        channel: &str,
        message_id: &MessageId,
        reaction: &str,
    ) -> ChatResult<()> {
        let body = serde_json::json!({
            "channel": channel,
            "timestamp": message_id.0,
            "name": reaction,
        });
        let _: serde_json::Value = self.api_post("reactions.add", &body).await?;
        Ok(())
    }

    async fn get_reactions(
        &self,
        channel: &str,
        message_id: &MessageId,
    ) -> ChatResult<Vec<Reaction>> {
        let resp: SlackReactionsResp = self
            .api_get(
                "reactions.get",
                &[("channel", channel), ("timestamp", &message_id.0)],
            )
            .await?;
        Ok(resp
            .message
            .reactions
            .unwrap_or_default()
            .into_iter()
            .map(|r| Reaction {
                name: r.name,
                users: r
                    .users
                    .into_iter()
                    .map(|uid| User {
                        id: uid.clone(),
                        name: uid,
                        display_name: None,
                    })
                    .collect(),
            })
            .collect())
    }

    async fn get_thread(&self, channel: &str, parent_id: &MessageId) -> ChatResult<Vec<Message>> {
        let resp: SlackRepliesResp = self
            .api_get(
                "conversations.replies",
                &[("channel", channel), ("ts", &parent_id.0)],
            )
            .await?;
        Ok(resp
            .messages
            .iter()
            .map(|m| slack_msg_to_message(channel, m))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_name() {
        let adapter = SlackAdapter::new("xoxb-test");
        assert_eq!(adapter.platform(), "slack");
    }

    #[test]
    fn with_client_constructor() {
        let client = Client::new();
        let adapter = SlackAdapter::with_client(client, "xoxb-test");
        assert_eq!(adapter.platform(), "slack");
    }

    #[test]
    fn slack_msg_to_message_basic() {
        let m = SlackMessage {
            ts: "1234567890.123456".into(),
            user: Some("U001".into()),
            text: Some("hello".into()),
            thread_ts: None,
            reply_count: None,
        };
        let msg = slack_msg_to_message("C001", &m);
        assert_eq!(msg.id, MessageId("1234567890.123456".into()));
        assert_eq!(msg.channel, "C001");
        assert_eq!(msg.text, "hello");
        assert_eq!(msg.author.id, "U001");
        assert!(msg.thread.is_none());
        assert_eq!(msg.timestamp, "1234567890.123456");
    }

    #[test]
    fn slack_msg_to_message_with_thread() {
        let m = SlackMessage {
            ts: "1234567890.999".into(),
            user: Some("U002".into()),
            text: Some("reply".into()),
            thread_ts: Some("1234567890.001".into()),
            reply_count: Some(3),
        };
        let msg = slack_msg_to_message("C002", &m);
        let thread = msg.thread.unwrap();
        assert_eq!(thread.parent_id, MessageId("1234567890.001".into()));
        assert_eq!(thread.reply_count, 3);
    }

    #[test]
    fn slack_msg_defaults_for_missing_fields() {
        let m = SlackMessage {
            ts: "100.0".into(),
            user: None,
            text: None,
            thread_ts: None,
            reply_count: None,
        };
        let msg = slack_msg_to_message("C003", &m);
        assert_eq!(msg.author.id, "");
        assert_eq!(msg.text, "");
    }

    #[test]
    fn check_slack_error_ok() {
        let body = serde_json::json!({"ok": true});
        assert!(check_slack_error(&body).is_ok());
    }

    #[test]
    fn check_slack_error_auth() {
        let body = serde_json::json!({"ok": false, "error": "invalid_auth"});
        let err = check_slack_error(&body).unwrap_err();
        assert!(matches!(err, ChatError::Auth(_)));
    }

    #[test]
    fn check_slack_error_channel_not_found() {
        let body = serde_json::json!({"ok": false, "error": "channel_not_found"});
        let err = check_slack_error(&body).unwrap_err();
        assert!(matches!(err, ChatError::ChannelNotFound(_)));
    }

    #[test]
    fn check_slack_error_rate_limited() {
        let body = serde_json::json!({"ok": false, "error": "ratelimited"});
        let err = check_slack_error(&body).unwrap_err();
        assert!(matches!(
            err,
            ChatError::RateLimited {
                retry_after_secs: 30
            }
        ));
    }

    #[test]
    fn check_slack_error_generic() {
        let body = serde_json::json!({"ok": false, "error": "too_many_attachments"});
        let err = check_slack_error(&body).unwrap_err();
        assert!(matches!(err, ChatError::Api(_)));
    }
}
