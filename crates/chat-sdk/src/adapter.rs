use async_trait::async_trait;

use crate::error::ChatResult;
use crate::model::{Channel, Message, MessageId, Reaction, SendMessage};

/// Core trait for chat service adapters.
///
/// Each platform (Slack, Discord, etc.) implements this trait
/// to provide a unified interface for chat operations.
#[async_trait]
pub trait ChatAdapter: Send + Sync {
    /// Returns the platform name (e.g., "slack", "discord").
    fn platform(&self) -> &str;

    /// Send a message to a channel.
    async fn send_message(&self, msg: SendMessage) -> ChatResult<MessageId>;

    /// Fetch messages from a channel.
    async fn get_messages(&self, channel: &str, limit: usize) -> ChatResult<Vec<Message>>;

    /// List available channels.
    async fn list_channels(&self) -> ChatResult<Vec<Channel>>;

    /// Add a reaction to a message.
    async fn add_reaction(
        &self,
        channel: &str,
        message_id: &MessageId,
        reaction: &str,
    ) -> ChatResult<()>;

    /// Get reactions on a message.
    async fn get_reactions(
        &self,
        channel: &str,
        message_id: &MessageId,
    ) -> ChatResult<Vec<Reaction>>;

    /// Get thread replies for a message.
    async fn get_thread(&self, channel: &str, parent_id: &MessageId) -> ChatResult<Vec<Message>>;

    /// Edit an existing message. Used for streaming LLM responses via Post+Edit pattern.
    async fn edit_message(
        &self,
        channel: &str,
        message_id: &MessageId,
        new_text: &str,
    ) -> ChatResult<()>;
}
