use serde::{Deserialize, Serialize};

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
}
