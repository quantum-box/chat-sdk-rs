//! Streaming message support via Post+Edit pattern.
//!
//! Provides [`StreamingMessage`] for incrementally updating a chat message,
//! enabling real-time display of LLM token-by-token output.
//!
//! # Example
//!
//! ```rust,ignore
//! use chat_sdk::{StreamingMessage, SlackAdapter};
//!
//! async fn example() -> chat_sdk::ChatResult<()> {
//!     let adapter = SlackAdapter::new("xoxb-token");
//!     let mut stream = StreamingMessage::new(&adapter, "C123456").await?;
//!
//!     for token in ["Hello", ", ", "world", "!"] {
//!         stream.push(token).await?;
//!     }
//!
//!     stream.finalize("Hello, world!").await?;
//!     Ok(())
//! }
//! ```

use std::time::{Duration, Instant};

use crate::adapter::ChatAdapter;
use crate::error::ChatResult;
use crate::model::{MessageId, SendMessage};

/// Default placeholder text shown while the stream initializes.
const DEFAULT_PLACEHOLDER: &str = "…";

/// Minimum interval between edit API calls to avoid rate limits.
const DEFAULT_MIN_EDIT_INTERVAL: Duration = Duration::from_millis(300);

/// A streaming message that is first posted and then updated via edits.
///
/// This implements the Post+Edit pattern: an initial message is sent with
/// placeholder text, then subsequent calls to [`push`](Self::push) or
/// [`update`](Self::update) edit the message content in place.
///
/// This is useful for displaying LLM streaming responses where tokens
/// arrive incrementally.
pub struct StreamingMessage<'a> {
    adapter: &'a dyn ChatAdapter,
    channel: String,
    message_id: MessageId,
    buffer: String,
    last_edit: Instant,
    min_interval: Duration,
    dirty: bool,
}

impl<'a> StreamingMessage<'a> {
    /// Post an initial placeholder message and return a streaming handle.
    pub async fn new(adapter: &'a dyn ChatAdapter, channel: &str) -> ChatResult<Self> {
        Self::with_placeholder(adapter, channel, DEFAULT_PLACEHOLDER).await
    }

    /// Post an initial message with custom placeholder text.
    pub async fn with_placeholder(
        adapter: &'a dyn ChatAdapter,
        channel: &str,
        placeholder: &str,
    ) -> ChatResult<Self> {
        let msg = SendMessage::text(channel, placeholder);
        let message_id = adapter.send_message(msg).await?;
        Ok(Self {
            adapter,
            channel: channel.to_string(),
            message_id,
            buffer: placeholder.to_string(),
            last_edit: Instant::now(),
            min_interval: DEFAULT_MIN_EDIT_INTERVAL,
            dirty: false,
        })
    }

    /// Post an initial placeholder message as a thread reply.
    pub async fn in_thread(
        adapter: &'a dyn ChatAdapter,
        channel: &str,
        thread_id: MessageId,
    ) -> ChatResult<Self> {
        let msg = SendMessage::text(channel, DEFAULT_PLACEHOLDER).in_thread(thread_id);
        let message_id = adapter.send_message(msg).await?;
        Ok(Self {
            adapter,
            channel: channel.to_string(),
            message_id,
            buffer: DEFAULT_PLACEHOLDER.to_string(),
            last_edit: Instant::now(),
            min_interval: DEFAULT_MIN_EDIT_INTERVAL,
            dirty: false,
        })
    }

    /// Set the minimum interval between edit API calls.
    ///
    /// Lower values give smoother updates but risk rate limiting.
    /// Default is 300ms.
    pub fn set_min_interval(&mut self, interval: Duration) {
        self.min_interval = interval;
    }

    /// Append text to the buffer and edit the message if enough time has elapsed.
    ///
    /// Tokens are always buffered immediately. The actual API edit is
    /// throttled to [`min_interval`](Self::set_min_interval) to avoid
    /// rate limits. Call [`flush`](Self::flush) to force an immediate update.
    pub async fn push(&mut self, token: &str) -> ChatResult<()> {
        if self.buffer == DEFAULT_PLACEHOLDER {
            self.buffer.clear();
        }
        self.buffer.push_str(token);
        self.dirty = true;
        self.maybe_flush().await
    }

    /// Replace the entire message content and edit if enough time has elapsed.
    pub async fn update(&mut self, text: &str) -> ChatResult<()> {
        self.buffer = text.to_string();
        self.dirty = true;
        self.maybe_flush().await
    }

    /// Force an immediate edit with the current buffer content.
    pub async fn flush(&mut self) -> ChatResult<()> {
        if self.dirty {
            self.adapter
                .edit_message(&self.channel, &self.message_id, &self.buffer)
                .await?;
            self.last_edit = Instant::now();
            self.dirty = false;
        }
        Ok(())
    }

    /// Finalize the streaming message with the given text.
    ///
    /// This always performs an edit regardless of throttling, ensuring
    /// the final message content is correct.
    pub async fn finalize(&mut self, final_text: &str) -> ChatResult<()> {
        self.buffer = final_text.to_string();
        self.dirty = true;
        self.flush().await
    }

    /// Finalize the message with the current buffer content.
    pub async fn done(&mut self) -> ChatResult<()> {
        self.flush().await
    }

    /// Return the message ID of the streaming message.
    pub fn message_id(&self) -> &MessageId {
        &self.message_id
    }

    /// Return the current buffer content.
    pub fn current_text(&self) -> &str {
        &self.buffer
    }

    async fn maybe_flush(&mut self) -> ChatResult<()> {
        if self.last_edit.elapsed() >= self.min_interval {
            self.flush().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_placeholder() {
        assert_eq!(DEFAULT_PLACEHOLDER, "…");
    }

    #[test]
    fn default_interval() {
        assert_eq!(DEFAULT_MIN_EDIT_INTERVAL, Duration::from_millis(300));
    }
}
