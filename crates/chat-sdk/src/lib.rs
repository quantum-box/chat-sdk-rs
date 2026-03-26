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
pub mod state;
pub mod streaming;
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
pub use state::{InMemoryStateAdapter, Session, StateAdapter};
pub use streaming::StreamingMessage;

#[cfg(feature = "redis")]
pub use state::RedisStateAdapter;
pub use webhook::{
    DiscordCommandOption, DiscordInteraction, DiscordInteractionData, DiscordInteractionResponse,
    DiscordInteractionResponseData, DiscordInteractionUser, DiscordMember, SlackEnvelope,
    SlackEvent, SlackWebhookHandler, WebhookConfig, WebhookHandler, WebhookResponse, WebhookServer,
    handler_router, router as webhook_router, start as start_webhook, verify_signature,
    verify_slack_signature,
};

#[cfg(feature = "discord")]
pub use webhook::{DiscordWebhookHandler, verify_discord_signature};

#[cfg(feature = "slack")]
pub use slack::SlackAdapter;

#[cfg(feature = "discord")]
pub use discord::DiscordAdapter;
#[cfg(feature = "discord")]
pub use discord::edit_message as discord_edit_message;
