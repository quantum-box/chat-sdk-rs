use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::adapter::ChatAdapter;
use crate::error::{ChatError, ChatResult};
use crate::model::{Channel, Message, MessageId, Reaction, SendMessage, Thread, User};

const BASE_URL: &str = "https://discord.com/api/v10";

/// Discord adapter implementing [`ChatAdapter`].
pub struct DiscordAdapter {
    client: Client,
    token: String,
}

impl DiscordAdapter {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            token: token.into(),
        }
    }

    fn auth_header(&self) -> String {
        if self.token.starts_with("Bot ") {
            self.token.clone()
        } else {
            format!("Bot {}", self.token)
        }
    }

    async fn api_get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> ChatResult<T> {
        let resp = self
            .client
            .get(format!("{BASE_URL}{path}"))
            .header("Authorization", self.auth_header())
            .query(query)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ChatError::Api(format!("discord {status}: {text}")));
        }
        let body = resp.json().await?;
        Ok(body)
    }

    async fn api_post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> ChatResult<T> {
        let resp = self
            .client
            .post(format!("{BASE_URL}{path}"))
            .header("Authorization", self.auth_header())
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ChatError::Api(format!("discord {status}: {text}")));
        }
        let body = resp.json().await?;
        Ok(body)
    }
}

// -- Discord API response types --

#[derive(Deserialize)]
struct DiscordMessage {
    id: String,
    channel_id: String,
    author: DiscordUser,
    content: String,
    timestamp: String,
    message_reference: Option<DiscordMessageReference>,
    reactions: Option<Vec<DiscordReaction>>,
}

#[derive(Deserialize)]
struct DiscordUser {
    id: String,
    username: String,
    global_name: Option<String>,
}

#[derive(Deserialize)]
struct DiscordMessageReference {
    message_id: Option<String>,
}

#[derive(Deserialize)]
struct DiscordChannel {
    id: String,
    name: Option<String>,
    topic: Option<String>,
    #[serde(rename = "type")]
    channel_type: u8,
}

#[derive(Deserialize)]
struct DiscordReaction {
    emoji: DiscordEmoji,
    #[allow(dead_code)]
    count: u32,
}

#[derive(Deserialize)]
struct DiscordEmoji {
    name: Option<String>,
}

#[derive(Deserialize)]
struct DiscordReactionUser {
    id: String,
    username: String,
    global_name: Option<String>,
}

// -- Helpers --

fn discord_msg_to_message(m: &DiscordMessage) -> Message {
    Message {
        id: MessageId(m.id.clone()),
        channel: m.channel_id.clone(),
        author: User {
            id: m.author.id.clone(),
            name: m.author.username.clone(),
            display_name: m.author.global_name.clone(),
        },
        text: m.content.clone(),
        thread: m.message_reference.as_ref().and_then(|r| {
            r.message_id.as_ref().map(|id| Thread {
                parent_id: MessageId(id.clone()),
                reply_count: 0,
            })
        }),
        timestamp: m.timestamp.clone(),
    }
}

// -- ChatAdapter impl --

#[async_trait]
impl ChatAdapter for DiscordAdapter {
    fn platform(&self) -> &str {
        "discord"
    }

    async fn send_message(&self, msg: SendMessage) -> ChatResult<MessageId> {
        let mut body = serde_json::json!({
            "content": msg.text,
        });
        if let Some(ref thread_id) = msg.thread_id {
            body["message_reference"] = serde_json::json!({
                "message_id": thread_id.0,
            });
        }
        if !msg.cards.is_empty() {
            let embeds: Vec<serde_json::Value> =
                msg.cards.iter().map(|c| c.to_discord_embed()).collect();
            body["embeds"] = serde_json::Value::Array(embeds);
        }
        let resp: DiscordMessage = self
            .api_post(&format!("/channels/{}/messages", msg.channel), &body)
            .await?;
        Ok(MessageId(resp.id))
    }

    async fn get_messages(&self, channel: &str, limit: usize) -> ChatResult<Vec<Message>> {
        let limit_str = limit.to_string();
        let msgs: Vec<DiscordMessage> = self
            .api_get(
                &format!("/channels/{channel}/messages"),
                &[("limit", &limit_str)],
            )
            .await?;
        Ok(msgs.iter().map(discord_msg_to_message).collect())
    }

    async fn list_channels(&self) -> ChatResult<Vec<Channel>> {
        let guild_id = std::env::var("CHAT_SDK_GUILD").map_err(|_| {
            ChatError::Other("CHAT_SDK_GUILD env var is required for Discord".into())
        })?;
        let channels: Vec<DiscordChannel> = self
            .api_get(&format!("/guilds/{guild_id}/channels"), &[])
            .await?;
        Ok(channels
            .into_iter()
            .filter(|c| c.channel_type == 0)
            .map(|c| Channel {
                id: c.id,
                name: c.name.unwrap_or_default(),
                topic: c.topic,
            })
            .collect())
    }

    async fn add_reaction(
        &self,
        channel: &str,
        message_id: &MessageId,
        reaction: &str,
    ) -> ChatResult<()> {
        let encoded = urlencoding::encode(reaction);
        let resp = self
            .client
            .put(format!(
                "{BASE_URL}/channels/{channel}/messages/{}/reactions/{encoded}/@me",
                message_id.0
            ))
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ChatError::Api(format!("discord {status}: {text}")));
        }
        Ok(())
    }

    async fn get_reactions(
        &self,
        channel: &str,
        message_id: &MessageId,
    ) -> ChatResult<Vec<Reaction>> {
        let msg: DiscordMessage = self
            .api_get(
                &format!("/channels/{channel}/messages/{}", message_id.0),
                &[],
            )
            .await?;
        let Some(reactions) = msg.reactions else {
            return Ok(vec![]);
        };
        let mut result = Vec::new();
        for r in reactions {
            let emoji_name = r.emoji.name.unwrap_or_default();
            let encoded = urlencoding::encode(&emoji_name);
            let users: Vec<DiscordReactionUser> = self
                .api_get(
                    &format!(
                        "/channels/{channel}/messages/{}/reactions/{encoded}",
                        message_id.0
                    ),
                    &[],
                )
                .await?;
            result.push(Reaction {
                name: emoji_name,
                users: users
                    .into_iter()
                    .map(|u| User {
                        id: u.id,
                        name: u.username,
                        display_name: u.global_name,
                    })
                    .collect(),
            });
        }
        Ok(result)
    }

    async fn get_thread(&self, channel: &str, _parent_id: &MessageId) -> ChatResult<Vec<Message>> {
        let msgs: Vec<DiscordMessage> = self
            .api_get(
                &format!("/channels/{channel}/messages"),
                &[("limit", "100")],
            )
            .await?;
        Ok(msgs.iter().map(discord_msg_to_message).collect())
    }
}
