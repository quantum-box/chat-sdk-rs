//! # chat-sdk
//!
//! A Rust chat service abstraction library with adapter pattern.
//! Supports multiple platforms (Slack, Discord, etc.) through a unified trait interface.

pub mod adapter;
pub mod card;
pub mod command;
pub mod error;
pub mod event;
pub mod format;
pub mod model;
pub mod oauth;
pub mod webhook;

#[cfg(feature = "slack")]
pub mod slack;

#[cfg(feature = "discord")]
pub mod discord;

pub use adapter::ChatAdapter;
pub use card::{Author, Card, CardBuilder, Color, Field};
pub use command::{CommandResponse, CommandRouter, ResponseBuilder, SlashCommand};
pub use error::{ChatError, ChatResult};
pub use event::{
    ChatEvent, EventHandlerBuilder, EventKind, EventRouter, MentionEvent, MessageDeletedEvent,
    ReactionEvent,
};
pub use format::{Document, MessageFormatter, Node, Platform};
pub use model::{Channel, Message, MessageId, Reaction, SendMessage, Thread, User};
pub use oauth::{OAuthConfig, TokenData, TokenStore};
pub use webhook::{
    SlackEnvelope, SlackEvent, WebhookConfig, WebhookServer, router as webhook_router,
    start as start_webhook, verify_signature,
};

#[cfg(feature = "slack")]
pub use slack::SlackAdapter;

#[cfg(feature = "discord")]
pub use discord::DiscordAdapter;
