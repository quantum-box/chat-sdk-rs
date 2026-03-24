//! # chat-sdk
//!
//! A Rust chat service abstraction library with adapter pattern.
//! Supports multiple platforms (Slack, Discord, etc.) through a unified trait interface.

pub mod adapter;
pub mod error;
pub mod model;
pub mod oauth;

pub use adapter::ChatAdapter;
pub use error::{ChatError, ChatResult};
pub use model::{Channel, Message, MessageId, Reaction, Thread, User};
