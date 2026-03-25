//! Event handling infrastructure for chat-sdk.
//!
//! Provides [`ChatEvent`] types, an [`EventRouter`] for dispatching events
//! to registered handlers, and an [`EventHandlerBuilder`] for ergonomic
//! callback registration.
//!
//! # Example
//!
//! ```rust
//! use chat_sdk::event::{EventRouter, ChatEvent};
//!
//! let router = EventRouter::builder()
//!     .on_message(|event| Box::pin(async move {
//!         if let ChatEvent::Message(msg) = &event {
//!             println!("Got message: {}", msg.text);
//!         }
//!         Ok(())
//!     }))
//!     .on_mention(|event| Box::pin(async move {
//!         println!("Mentioned!");
//!         Ok(())
//!     }))
//!     .build();
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::ChatResult;
use crate::model::{Message, MessageId, User};

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// Events that can be received from a chat platform.
#[derive(Debug, Clone)]
pub enum ChatEvent {
    /// A new message was posted in a channel.
    Message(Message),
    /// The bot or a user was mentioned in a message.
    Mention(MentionEvent),
    /// A reaction was added to a message.
    ReactionAdded(ReactionEvent),
    /// A reaction was removed from a message.
    ReactionRemoved(ReactionEvent),
    /// A message was updated/edited.
    MessageUpdated(Message),
    /// A message was deleted.
    MessageDeleted(MessageDeletedEvent),
}

/// A mention event with the originating message and mentioned user.
#[derive(Debug, Clone)]
pub struct MentionEvent {
    pub message: Message,
    pub mentioned_user: User,
}

/// A reaction event (add or remove).
#[derive(Debug, Clone)]
pub struct ReactionEvent {
    pub channel: String,
    pub message_id: MessageId,
    pub reaction: String,
    pub user: User,
}

/// A message-deleted event.
#[derive(Debug, Clone)]
pub struct MessageDeletedEvent {
    pub channel: String,
    pub message_id: MessageId,
    pub timestamp: String,
}

/// The kind/discriminant of a [`ChatEvent`], without payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventKind {
    Message,
    Mention,
    ReactionAdded,
    ReactionRemoved,
    MessageUpdated,
    MessageDeleted,
}

impl ChatEvent {
    /// Returns the [`EventKind`] discriminant for this event.
    pub fn kind(&self) -> EventKind {
        match self {
            ChatEvent::Message(_) => EventKind::Message,
            ChatEvent::Mention(_) => EventKind::Mention,
            ChatEvent::ReactionAdded(_) => EventKind::ReactionAdded,
            ChatEvent::ReactionRemoved(_) => EventKind::ReactionRemoved,
            ChatEvent::MessageUpdated(_) => EventKind::MessageUpdated,
            ChatEvent::MessageDeleted(_) => EventKind::MessageDeleted,
        }
    }
}

// ---------------------------------------------------------------------------
// Handler type
// ---------------------------------------------------------------------------

/// A boxed, async event handler function.
///
/// Handlers receive a [`ChatEvent`] and return a [`ChatResult<()>`].
pub type BoxedHandler =
    Arc<dyn Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>> + Send + Sync>;

// ---------------------------------------------------------------------------
// EventRouter
// ---------------------------------------------------------------------------

/// Routes incoming [`ChatEvent`]s to registered handler callbacks.
///
/// Create one via [`EventRouter::builder()`].
pub struct EventRouter {
    handlers: Vec<(EventKind, BoxedHandler)>,
    catch_all: Vec<BoxedHandler>,
}

impl std::fmt::Debug for EventRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventRouter")
            .field("handler_count", &self.handlers.len())
            .field("catch_all_count", &self.catch_all.len())
            .finish()
    }
}

impl EventRouter {
    /// Create a new [`EventHandlerBuilder`].
    pub fn builder() -> EventHandlerBuilder {
        EventHandlerBuilder::new()
    }

    /// Dispatch an event to all matching handlers.
    ///
    /// Runs kind-specific handlers first, then catch-all handlers, in
    /// registration order. Returns the first error encountered, but still
    /// attempts all handlers.
    pub async fn dispatch(&self, event: ChatEvent) -> ChatResult<()> {
        let kind = event.kind();
        let mut first_err: Option<crate::error::ChatError> = None;

        for (k, handler) in &self.handlers {
            if *k == kind {
                if let Err(e) = handler(event.clone()).await {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }

        for handler in &self.catch_all {
            if let Err(e) = handler(event.clone()).await {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }

        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Returns the number of registered handlers (kind-specific + catch-all).
    pub fn handler_count(&self) -> usize {
        self.handlers.len() + self.catch_all.len()
    }

    /// Returns true if no handlers are registered.
    pub fn is_empty(&self) -> bool {
        self.handler_count() == 0
    }
}

// ---------------------------------------------------------------------------
// EventHandlerBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing an [`EventRouter`] with registered callbacks.
pub struct EventHandlerBuilder {
    handlers: Vec<(EventKind, BoxedHandler)>,
    catch_all: Vec<BoxedHandler>,
}

impl EventHandlerBuilder {
    /// Create a new, empty builder.
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            catch_all: Vec::new(),
        }
    }

    /// Register a handler for [`ChatEvent::Message`] events.
    pub fn on_message<F>(self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.on(EventKind::Message, handler)
    }

    /// Register a handler for [`ChatEvent::Mention`] events.
    pub fn on_mention<F>(self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.on(EventKind::Mention, handler)
    }

    /// Register a handler for [`ChatEvent::ReactionAdded`] events.
    pub fn on_reaction_added<F>(self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.on(EventKind::ReactionAdded, handler)
    }

    /// Register a handler for [`ChatEvent::ReactionRemoved`] events.
    pub fn on_reaction_removed<F>(self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.on(EventKind::ReactionRemoved, handler)
    }

    /// Register a handler for [`ChatEvent::MessageUpdated`] events.
    pub fn on_message_updated<F>(self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.on(EventKind::MessageUpdated, handler)
    }

    /// Register a handler for [`ChatEvent::MessageDeleted`] events.
    pub fn on_message_deleted<F>(self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.on(EventKind::MessageDeleted, handler)
    }

    /// Register a handler for a specific [`EventKind`].
    pub fn on<F>(mut self, kind: EventKind, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.handlers.push((kind, Arc::new(handler)));
        self
    }

    /// Register a catch-all handler that receives every event.
    pub fn on_any<F>(mut self, handler: F) -> Self
    where
        F: Fn(ChatEvent) -> Pin<Box<dyn Future<Output = ChatResult<()>> + Send>>
            + Send
            + Sync
            + 'static,
    {
        self.catch_all.push(Arc::new(handler));
        self
    }

    /// Build the [`EventRouter`].
    pub fn build(self) -> EventRouter {
        EventRouter {
            handlers: self.handlers,
            catch_all: self.catch_all,
        }
    }
}

impl Default for EventHandlerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Message, MessageId, User};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_message() -> Message {
        Message {
            id: MessageId("msg-1".into()),
            channel: "general".into(),
            author: User {
                id: "u1".into(),
                name: "alice".into(),
                display_name: Some("Alice".into()),
            },
            text: "hello world".into(),
            thread: None,
            timestamp: "1700000000.000000".into(),
        }
    }

    fn test_user() -> User {
        User {
            id: "u2".into(),
            name: "bot".into(),
            display_name: Some("Bot".into()),
        }
    }

    // -- ChatEvent::kind -------------------------------------------------

    #[test]
    fn event_kind_message() {
        let event = ChatEvent::Message(test_message());
        assert_eq!(event.kind(), EventKind::Message);
    }

    #[test]
    fn event_kind_mention() {
        let event = ChatEvent::Mention(MentionEvent {
            message: test_message(),
            mentioned_user: test_user(),
        });
        assert_eq!(event.kind(), EventKind::Mention);
    }

    #[test]
    fn event_kind_reaction_added() {
        let event = ChatEvent::ReactionAdded(ReactionEvent {
            channel: "general".into(),
            message_id: MessageId("msg-1".into()),
            reaction: "thumbsup".into(),
            user: test_user(),
        });
        assert_eq!(event.kind(), EventKind::ReactionAdded);
    }

    #[test]
    fn event_kind_reaction_removed() {
        let event = ChatEvent::ReactionRemoved(ReactionEvent {
            channel: "general".into(),
            message_id: MessageId("msg-1".into()),
            reaction: "thumbsup".into(),
            user: test_user(),
        });
        assert_eq!(event.kind(), EventKind::ReactionRemoved);
    }

    #[test]
    fn event_kind_message_updated() {
        let event = ChatEvent::MessageUpdated(test_message());
        assert_eq!(event.kind(), EventKind::MessageUpdated);
    }

    #[test]
    fn event_kind_message_deleted() {
        let event = ChatEvent::MessageDeleted(MessageDeletedEvent {
            channel: "general".into(),
            message_id: MessageId("msg-1".into()),
            timestamp: "1700000000.000000".into(),
        });
        assert_eq!(event.kind(), EventKind::MessageDeleted);
    }

    // -- EventRouter dispatch ---------------------------------------------

    #[tokio::test]
    async fn dispatch_calls_matching_handler() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let router = EventRouter::builder()
            .on_message(move |_event| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        let event = ChatEvent::Message(test_message());
        router.dispatch(event).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn dispatch_skips_non_matching_handler() {
        let msg_count = Arc::new(AtomicUsize::new(0));
        let mention_count = Arc::new(AtomicUsize::new(0));
        let mc = msg_count.clone();
        let mnc = mention_count.clone();

        let router = EventRouter::builder()
            .on_message(move |_| {
                let mc = mc.clone();
                Box::pin(async move {
                    mc.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .on_mention(move |_| {
                let mnc = mnc.clone();
                Box::pin(async move {
                    mnc.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        let event = ChatEvent::Message(test_message());
        router.dispatch(event).await.unwrap();

        assert_eq!(msg_count.load(Ordering::SeqCst), 1);
        assert_eq!(mention_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dispatch_calls_multiple_handlers_for_same_kind() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c1 = counter.clone();
        let c2 = counter.clone();

        let router = EventRouter::builder()
            .on_message(move |_| {
                let c = c1.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .on_message(move |_| {
                let c = c2.clone();
                Box::pin(async move {
                    c.fetch_add(10, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        router
            .dispatch(ChatEvent::Message(test_message()))
            .await
            .unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 11);
    }

    #[tokio::test]
    async fn dispatch_catch_all_receives_all_events() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let router = EventRouter::builder()
            .on_any(move |_| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        router
            .dispatch(ChatEvent::Message(test_message()))
            .await
            .unwrap();
        router
            .dispatch(ChatEvent::Mention(MentionEvent {
                message: test_message(),
                mentioned_user: test_user(),
            }))
            .await
            .unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn dispatch_returns_first_error() {
        let router = EventRouter::builder()
            .on_message(|_| Box::pin(async { Err(crate::error::ChatError::Other("boom".into())) }))
            .build();

        let result = router.dispatch(ChatEvent::Message(test_message())).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("boom"));
    }

    #[tokio::test]
    async fn dispatch_continues_after_error() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let router = EventRouter::builder()
            .on_message(|_| Box::pin(async { Err(crate::error::ChatError::Other("fail".into())) }))
            .on_message(move |_| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        let _ = router.dispatch(ChatEvent::Message(test_message())).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    // -- Builder / Router metadata ----------------------------------------

    #[test]
    fn handler_count_and_is_empty() {
        let empty = EventRouter::builder().build();
        assert!(empty.is_empty());
        assert_eq!(empty.handler_count(), 0);

        let router = EventRouter::builder()
            .on_message(|_| Box::pin(async { Ok(()) }))
            .on_mention(|_| Box::pin(async { Ok(()) }))
            .on_any(|_| Box::pin(async { Ok(()) }))
            .build();
        assert_eq!(router.handler_count(), 3);
        assert!(!router.is_empty());
    }

    #[test]
    fn router_debug_impl() {
        let router = EventRouter::builder()
            .on_message(|_| Box::pin(async { Ok(()) }))
            .build();
        let debug = format!("{:?}", router);
        assert!(debug.contains("EventRouter"));
        assert!(debug.contains("handler_count"));
    }

    #[test]
    fn builder_default_is_empty() {
        let builder = EventHandlerBuilder::default();
        let router = builder.build();
        assert!(router.is_empty());
    }

    #[tokio::test]
    async fn on_reaction_added_handler() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let router = EventRouter::builder()
            .on_reaction_added(move |_| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        let event = ChatEvent::ReactionAdded(ReactionEvent {
            channel: "general".into(),
            message_id: MessageId("msg-1".into()),
            reaction: "thumbsup".into(),
            user: test_user(),
        });
        router.dispatch(event).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn on_generic_kind_handler() {
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();

        let router = EventRouter::builder()
            .on(EventKind::MessageDeleted, move |_| {
                let c = c.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            })
            .build();

        let event = ChatEvent::MessageDeleted(MessageDeletedEvent {
            channel: "general".into(),
            message_id: MessageId("msg-1".into()),
            timestamp: "1700000000.000000".into(),
        });
        router.dispatch(event).await.unwrap();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
