use serde::{Deserialize, Serialize};

use crate::card::Card;

/// Unique identifier for a message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub String);

/// A chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel: String,
    pub author: User,
    pub text: String,
    pub thread: Option<Thread>,
    pub timestamp: String,
}

/// A chat user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
}

/// A chat channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub topic: Option<String>,
}

/// A message thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub parent_id: MessageId,
    pub reply_count: u32,
}

/// A reaction on a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub name: String,
    pub users: Vec<User>,
}

/// Parameters for sending a message.
#[derive(Debug, Clone)]
pub struct SendMessage {
    pub channel: String,
    pub text: String,
    pub thread_id: Option<MessageId>,
    /// Optional rich message cards attached to this message.
    pub cards: Vec<Card>,
}

impl SendMessage {
    /// Create a plain text message.
    pub fn text(channel: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            channel: channel.into(),
            text: text.into(),
            thread_id: None,
            cards: Vec::new(),
        }
    }

    /// Create a message with a single card.
    pub fn card(channel: impl Into<String>, card: Card) -> Self {
        Self {
            channel: channel.into(),
            text: String::new(),
            thread_id: None,
            cards: vec![card],
        }
    }

    /// Set the thread ID for replying to a thread.
    pub fn in_thread(mut self, thread_id: MessageId) -> Self {
        self.thread_id = Some(thread_id);
        self
    }

    /// Attach additional cards to the message.
    pub fn with_cards(mut self, cards: Vec<Card>) -> Self {
        self.cards = cards;
        self
    }
}
