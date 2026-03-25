//! # chat-sdk
//!
//! A Rust chat service abstraction library with adapter pattern.
//! Supports multiple platforms (Slack, Discord, etc.) through a unified trait interface.

pub mod adapter;
pub mod error;
pub mod event;
pub mod model;
pub mod oauth;

#[cfg(feature = "slack")]
pub mod slack;

#[cfg(feature = "discord")]
pub mod discord;

pub use adapter::ChatAdapter;
pub use error::{ChatError, ChatResult};
pub use event::{
    ChatEvent, EventHandlerBuilder, EventKind, EventRouter, MentionEvent, MessageDeletedEvent,
    ReactionEvent,
};
pub use model::{Channel, Message, MessageId, Reaction, SendMessage, Thread, User};
pub use oauth::{OAuthConfig, TokenData, TokenStore};

#[cfg(feature = "slack")]
pub use slack::SlackAdapter;

#[cfg(feature = "discord")]
pub use discord::DiscordAdapter;
