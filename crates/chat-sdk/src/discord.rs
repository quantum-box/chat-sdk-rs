use async_trait::async_trait;
use serenity::all::{
    ChannelId, ChannelType, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage,
    EditMessage, GetMessages, GuildId, Http, MessageId as SerenityMessageId, ReactionType,
};

use crate::adapter::ChatAdapter;
use crate::card::Color;
use crate::error::{ChatError, ChatResult};
use crate::model::{Channel, Message, MessageId, Reaction, SendMessage, Thread, User};

/// Discord adapter implementing [`ChatAdapter`] via the serenity crate.
pub struct DiscordAdapter {
    http: Http,
}

impl DiscordAdapter {
    /// Create a new Discord adapter with the given bot token.
    pub fn new(token: impl Into<String>) -> Self {
        let token = token.into();
        let prefixed = if token.starts_with("Bot ") {
            token
        } else {
            format!("Bot {token}")
        };
        Self {
            http: Http::new(&prefixed),
        }
    }

    /// Create a new Discord adapter from an existing serenity [`Http`] client.
    pub fn with_http(http: Http) -> Self {
        Self { http }
    }

    /// Return a reference to the underlying serenity [`Http`] client.
    pub fn http(&self) -> &Http {
        &self.http
    }

    fn guild_id() -> ChatResult<GuildId> {
        let id: u64 = std::env::var("CHAT_SDK_GUILD")
            .map_err(|_| ChatError::Other("CHAT_SDK_GUILD env var is required for Discord".into()))?
            .parse()
            .map_err(|_| ChatError::Other("CHAT_SDK_GUILD must be a valid u64".into()))?;
        Ok(GuildId::new(id))
    }

    fn parse_channel_id(s: &str) -> ChatResult<ChannelId> {
        let id: u64 = s
            .parse()
            .map_err(|_| ChatError::Other(format!("invalid channel id: {s}")))?;
        Ok(ChannelId::new(id))
    }

    fn parse_message_id(id: &MessageId) -> ChatResult<SerenityMessageId> {
        let n: u64 =
            id.0.parse()
                .map_err(|_| ChatError::Other(format!("invalid message id: {}", id.0)))?;
        Ok(SerenityMessageId::new(n))
    }
}

fn card_to_embed(card: &crate::card::Card) -> CreateEmbed {
    let mut embed = CreateEmbed::new();
    if let Some(ref title) = card.title {
        embed = embed.title(title);
    }
    if let Some(ref desc) = card.description {
        embed = embed.description(desc);
    }
    if let Some(color) = card.color {
        let color_int = match color {
            Color::Good => 0x2EB67D,
            Color::Warning => 0xECB22E,
            Color::Danger => 0xE01E5A,
            Color::Custom(c) => c,
        };
        embed = embed.colour(color_int);
    }
    for field in &card.fields {
        embed = embed.field(&field.name, &field.value, field.inline);
    }
    if let Some(ref author) = card.author {
        let mut a = CreateEmbedAuthor::new(&author.name);
        if let Some(ref url) = author.url {
            a = a.url(url);
        }
        if let Some(ref icon) = author.icon_url {
            a = a.icon_url(icon);
        }
        embed = embed.author(a);
    }
    if let Some(ref url) = card.image_url {
        embed = embed.image(url);
    }
    if let Some(ref url) = card.thumbnail_url {
        embed = embed.thumbnail(url);
    }
    if let Some(ref footer_text) = card.footer {
        let mut f = CreateEmbedFooter::new(footer_text);
        if let Some(ref icon) = card.footer_icon_url {
            f = f.icon_url(icon);
        }
        embed = embed.footer(f);
    }
    embed
}

fn serenity_msg_to_message(m: &serenity::all::Message) -> Message {
    Message {
        id: MessageId(m.id.to_string()),
        channel: m.channel_id.to_string(),
        author: User {
            id: m.author.id.to_string(),
            name: m.author.name.clone(),
            display_name: m.author.global_name.clone(),
        },
        text: m.content.clone(),
        thread: m.message_reference.as_ref().and_then(|r| {
            r.message_id.map(|id| Thread {
                parent_id: MessageId(id.to_string()),
                reply_count: 0,
            })
        }),
        timestamp: m.timestamp.to_string(),
    }
}

fn map_serenity_err(e: serenity::Error) -> ChatError {
    match e {
        serenity::Error::Http(ref http_err) => {
            let msg = http_err.to_string();
            if msg.contains("401") || msg.contains("403") {
                ChatError::Auth(msg)
            } else if msg.contains("404") {
                ChatError::ChannelNotFound(msg)
            } else if msg.contains("429") {
                ChatError::RateLimited {
                    retry_after_secs: 5,
                }
            } else {
                ChatError::Api(format!("discord: {msg}"))
            }
        }
        _ => ChatError::Api(format!("discord: {e}")),
    }
}

#[async_trait]
impl ChatAdapter for DiscordAdapter {
    fn platform(&self) -> &str {
        "discord"
    }

    async fn send_message(&self, msg: SendMessage) -> ChatResult<MessageId> {
        let channel_id = Self::parse_channel_id(&msg.channel)?;

        let mut builder = CreateMessage::new().content(&msg.text);

        if let Some(ref thread_id) = msg.thread_id {
            let mid = Self::parse_message_id(thread_id)?;
            builder = builder.reference_message((channel_id, mid));
        }

        if !msg.cards.is_empty() {
            let embeds: Vec<CreateEmbed> = msg.cards.iter().map(card_to_embed).collect();
            builder = builder.embeds(embeds);
        }

        let sent = channel_id
            .send_message(&self.http, builder)
            .await
            .map_err(map_serenity_err)?;

        Ok(MessageId(sent.id.to_string()))
    }

    async fn get_messages(&self, channel: &str, limit: usize) -> ChatResult<Vec<Message>> {
        let channel_id = Self::parse_channel_id(channel)?;
        let limit = limit.min(100) as u8;

        let builder = GetMessages::new().limit(limit);
        let msgs = channel_id
            .messages(&self.http, builder)
            .await
            .map_err(map_serenity_err)?;

        Ok(msgs.iter().map(serenity_msg_to_message).collect())
    }

    async fn list_channels(&self) -> ChatResult<Vec<Channel>> {
        let guild_id = Self::guild_id()?;

        let channels = guild_id
            .channels(&self.http)
            .await
            .map_err(map_serenity_err)?;

        Ok(channels
            .into_values()
            .filter(|c| c.kind == ChannelType::Text)
            .map(|c| Channel {
                id: c.id.to_string(),
                name: c.name.clone(),
                topic: c.topic.clone(),
            })
            .collect())
    }

    async fn add_reaction(
        &self,
        channel: &str,
        message_id: &MessageId,
        reaction: &str,
    ) -> ChatResult<()> {
        let channel_id = Self::parse_channel_id(channel)?;
        let msg_id = Self::parse_message_id(message_id)?;
        let reaction_type = ReactionType::Unicode(reaction.to_string());

        self.http
            .create_reaction(channel_id, msg_id, &reaction_type)
            .await
            .map_err(map_serenity_err)?;

        Ok(())
    }

    async fn get_reactions(
        &self,
        channel: &str,
        message_id: &MessageId,
    ) -> ChatResult<Vec<Reaction>> {
        let channel_id = Self::parse_channel_id(channel)?;
        let msg_id = Self::parse_message_id(message_id)?;

        let msg = channel_id
            .message(&self.http, msg_id)
            .await
            .map_err(map_serenity_err)?;

        let mut result = Vec::new();
        for r in &msg.reactions {
            let emoji_str = match &r.reaction_type {
                ReactionType::Unicode(s) => s.clone(),
                ReactionType::Custom { name, .. } => name.clone().unwrap_or_default(),
                _ => continue,
            };

            let users = self
                .http
                .get_reaction_users(channel_id, msg_id, &r.reaction_type, 100, None)
                .await
                .map_err(map_serenity_err)?;

            result.push(Reaction {
                name: emoji_str,
                users: users
                    .into_iter()
                    .map(|u| User {
                        id: u.id.to_string(),
                        name: u.name.clone(),
                        display_name: u.global_name.clone(),
                    })
                    .collect(),
            });
        }
        Ok(result)
    }

    async fn get_thread(&self, channel: &str, _parent_id: &MessageId) -> ChatResult<Vec<Message>> {
        // Discord threads are channels; fall back to fetching recent messages.
        let channel_id = Self::parse_channel_id(channel)?;

        let builder = GetMessages::new().limit(100);
        let msgs = channel_id
            .messages(&self.http, builder)
            .await
            .map_err(map_serenity_err)?;

        Ok(msgs.iter().map(serenity_msg_to_message).collect())
    }
}

/// Edit an existing Discord message. Useful for streaming LLM responses.
pub async fn edit_message(
    http: &Http,
    channel: &str,
    message_id: &MessageId,
    new_text: &str,
) -> ChatResult<()> {
    let channel_id = DiscordAdapter::parse_channel_id(channel)?;
    let msg_id = DiscordAdapter::parse_message_id(message_id)?;
    let builder = EditMessage::new().content(new_text);
    channel_id
        .edit_message(&http, msg_id, builder)
        .await
        .map_err(map_serenity_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_name() {
        let adapter = DiscordAdapter::new("test-token");
        assert_eq!(adapter.platform(), "discord");
    }

    #[test]
    fn parse_channel_id_valid() {
        let id = DiscordAdapter::parse_channel_id("123456789").unwrap();
        assert_eq!(id, ChannelId::new(123456789));
    }

    #[test]
    fn parse_channel_id_invalid() {
        assert!(DiscordAdapter::parse_channel_id("abc").is_err());
    }

    #[test]
    fn parse_message_id_valid() {
        let id = DiscordAdapter::parse_message_id(&MessageId("987654321".into())).unwrap();
        assert_eq!(id, SerenityMessageId::new(987654321));
    }

    #[test]
    fn parse_message_id_invalid() {
        assert!(DiscordAdapter::parse_message_id(&MessageId("abc".into())).is_err());
    }

    #[test]
    fn card_to_embed_basic() {
        let card = crate::card::CardBuilder::new()
            .title("Test")
            .description("Description")
            .color(Color::Good)
            .build();
        let _ = card_to_embed(&card);
    }

    #[test]
    fn card_to_embed_with_fields() {
        let card = crate::card::CardBuilder::new()
            .title("Deploy")
            .color(Color::Warning)
            .field("Status", "Running", true)
            .field("Region", "us-east-1", false)
            .build();
        let _ = card_to_embed(&card);
    }

    #[test]
    fn card_to_embed_full() {
        let card = crate::card::CardBuilder::new()
            .title("Full Card")
            .description("desc")
            .color(Color::Custom(0xFF0000))
            .author_full(
                "Author",
                Some("https://example.com".to_string()),
                Some("https://example.com/icon.png".to_string()),
            )
            .image("https://example.com/image.png")
            .thumbnail("https://example.com/thumb.png")
            .footer("Footer text")
            .footer_icon("https://example.com/footer.png")
            .build();
        let _ = card_to_embed(&card);
    }

    #[test]
    fn map_serenity_err_generic() {
        let err = map_serenity_err(serenity::Error::Other("test"));
        assert!(matches!(err, ChatError::Api(_)));
    }

    #[test]
    fn guild_id_missing_env() {
        unsafe { std::env::remove_var("CHAT_SDK_GUILD") };
        assert!(DiscordAdapter::guild_id().is_err());
    }
}
