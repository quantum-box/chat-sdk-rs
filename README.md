# chat-sdk-rs

[![CI](https://github.com/quantum-box/chat-sdk-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/quantum-box/chat-sdk-rs/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

Rust Chat Service Abstraction Library with adapter pattern. Provides a unified `ChatAdapter` trait to interact with multiple chat platforms (Slack, Discord, etc.) through a single API, plus a CLI for quick operations and OAuth helpers for authentication.

## Features

- **Multi-platform** - Unified `ChatAdapter` trait for Slack, Discord, and more
- **Type-safe models** - `Message`, `Channel`, `User`, `Thread`, `Reaction` with serde support
- **OAuth2 built-in** - Authorization URL generation and CSRF protection via `OAuthConfig`
- **CLI tool** - Send messages, list channels, and authenticate from the terminal
- **AI Agent ready** - Structured trait design for exposing chat operations via MCP Tools
- **Async-first** - Built on `tokio` and `async-trait`

## Installation

### Requirements

- Rust 1.85.0 or later (edition 2024)

### Library

Add to your `Cargo.toml`:

```toml
[dependencies]
chat-sdk = { git = "https://github.com/quantum-box/chat-sdk-rs" }
```

Feature flags (both enabled by default):

```toml
# Enable only Slack support
chat-sdk = { git = "https://github.com/quantum-box/chat-sdk-rs", default-features = false, features = ["slack"] }
```

| Feature   | Description            |
|-----------|------------------------|
| `slack`   | Slack adapter support  |
| `discord` | Discord adapter support|

### CLI

```bash
cargo install --git https://github.com/quantum-box/chat-sdk-rs chat-sdk-cli
```

Or build from source:

```bash
git clone https://github.com/quantum-box/chat-sdk-rs.git
cd chat-sdk-rs
cargo build --release
# Binary: target/release/chat-sdk
```

## Library Usage

### Implementing a ChatAdapter

The core abstraction is the `ChatAdapter` trait. Implement it for each chat platform:

```rust
use async_trait::async_trait;
use chat_sdk::{ChatAdapter, ChatError, ChatResult, Channel, Message, MessageId};
use chat_sdk::model::{Reaction, SendMessage};

pub struct SlackAdapter {
    token: String,
}

impl SlackAdapter {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl ChatAdapter for SlackAdapter {
    fn platform(&self) -> &str {
        "slack"
    }

    async fn send_message(&self, msg: SendMessage) -> ChatResult<MessageId> {
        // Call Slack API to post a message
        // msg.channel - target channel name or ID
        // msg.text    - message body
        // msg.thread_id - optional parent MessageId for thread replies
        todo!()
    }

    async fn get_messages(&self, channel: &str, limit: usize) -> ChatResult<Vec<Message>> {
        // Fetch recent messages from a Slack channel
        todo!()
    }

    async fn list_channels(&self) -> ChatResult<Vec<Channel>> {
        // List all available Slack channels
        todo!()
    }

    async fn add_reaction(
        &self,
        channel: &str,
        message_id: &MessageId,
        reaction: &str,
    ) -> ChatResult<()> {
        // Add an emoji reaction to a message
        todo!()
    }

    async fn get_reactions(
        &self,
        channel: &str,
        message_id: &MessageId,
    ) -> ChatResult<Vec<Reaction>> {
        // Get all reactions on a message
        todo!()
    }

    async fn get_thread(
        &self,
        channel: &str,
        parent_id: &MessageId,
    ) -> ChatResult<Vec<Message>> {
        // Get all replies in a thread
        todo!()
    }
}
```

### Sending a Message

```rust
use chat_sdk::model::SendMessage;

let adapter = SlackAdapter::new("xoxb-your-token".into());

// Send to a channel
let msg = SendMessage {
    channel: "general".into(),
    text: "Hello from chat-sdk!".into(),
    thread_id: None,
};
let message_id = adapter.send_message(msg).await?;

// Reply to a thread
let reply = SendMessage {
    channel: "general".into(),
    text: "Thread reply".into(),
    thread_id: Some(message_id),
};
adapter.send_message(reply).await?;
```

### Working with Messages and Channels

```rust
// List channels
let channels = adapter.list_channels().await?;
for ch in &channels {
    println!("#{} ({}): {:?}", ch.name, ch.id, ch.topic);
}

// Fetch recent messages
let messages = adapter.get_messages("general", 10).await?;
for msg in &messages {
    println!("[{}] {}: {}", msg.timestamp, msg.author.name, msg.text);
}

// Reactions
adapter.add_reaction("general", &messages[0].id, "thumbsup").await?;
let reactions = adapter.get_reactions("general", &messages[0].id).await?;

// Threads
let replies = adapter.get_thread("general", &messages[0].id).await?;
```

### Error Handling

All operations return `ChatResult<T>`, which is `Result<T, ChatError>`:

```rust
use chat_sdk::{ChatError, ChatResult};

match adapter.send_message(msg).await {
    Ok(id) => println!("Sent: {:?}", id),
    Err(ChatError::Auth(reason)) => eprintln!("Auth failed: {reason}"),
    Err(ChatError::RateLimited { retry_after_secs }) => {
        eprintln!("Rate limited, retry after {retry_after_secs}s");
    }
    Err(ChatError::ChannelNotFound(ch)) => eprintln!("No such channel: {ch}"),
    Err(ChatError::Network(e)) => eprintln!("Network error: {e}"),
    Err(e) => eprintln!("Error: {e}"),
}
```

### Using the Adapter as a Trait Object

Because `ChatAdapter` is `Send + Sync`, it works as a dynamic trait object:

```rust
use chat_sdk::ChatAdapter;

async fn broadcast(adapters: &[Box<dyn ChatAdapter>], text: &str) {
    for adapter in adapters {
        let msg = SendMessage {
            channel: "general".into(),
            text: text.into(),
            thread_id: None,
        };
        match adapter.send_message(msg).await {
            Ok(_) => println!("Sent on {}", adapter.platform()),
            Err(e) => eprintln!("Failed on {}: {e}", adapter.platform()),
        }
    }
}
```

## CLI Usage

The CLI binary is `chat-sdk`.

### Send a message

```bash
chat-sdk send --platform slack --channel general "Hello, world!"
```

Reply to a thread:

```bash
chat-sdk send --platform slack --channel general --thread 1234567890.123456 "Thread reply"
```

### List channels

```bash
chat-sdk channels --platform slack
```

### Authenticate via OAuth

```bash
chat-sdk auth slack
```

### Enable debug logging

```bash
RUST_LOG=debug chat-sdk send --platform slack --channel general "Hello"
```

## Supported Platforms

| Platform | Status   | Feature Flag |
|----------|----------|--------------|
| Slack    | Planned  | `slack`      |
| Discord  | Planned  | `discord`    |
| Teams    | Future   | -            |
| LINE     | Future   | -            |

## OAuth Setup

`chat-sdk` provides `OAuthConfig` for generating authorization URLs with CSRF protection.

### Configuration

```rust
use chat_sdk::oauth::{OAuthConfig, AuthorizationRequest};

let config = OAuthConfig {
    client_id: "your-client-id".into(),
    client_secret: "your-client-secret".into(),
    auth_url: "https://slack.com/oauth/v2/authorize".into(),
    token_url: "https://slack.com/api/oauth.v2.access".into(),
    redirect_url: "http://localhost:8080/callback".into(),
    scopes: vec![
        "channels:read".into(),
        "chat:write".into(),
        "reactions:read".into(),
        "reactions:write".into(),
    ],
};

let auth_request = config.authorize_url()?;
println!("Open this URL to authorize: {}", auth_request.url);
println!("CSRF token (verify on callback): {}", auth_request.csrf_token);
```

### Platform-specific OAuth URLs

| Platform | Auth URL | Token URL |
|----------|----------|-----------|
| Slack | `https://slack.com/oauth/v2/authorize` | `https://slack.com/api/oauth.v2.access` |
| Discord | `https://discord.com/api/oauth2/authorize` | `https://discord.com/api/oauth2/token` |

## AI Agent Integration

The `ChatAdapter` trait is designed to be exposed as tools for AI agents (e.g., via MCP - Model Context Protocol). Each trait method maps naturally to a tool:

| Tool Name | ChatAdapter Method | Description |
|-----------|-------------------|-------------|
| `send_message` | `send_message(SendMessage)` | Send a message to a channel |
| `get_messages` | `get_messages(channel, limit)` | Read recent messages |
| `list_channels` | `list_channels()` | List available channels |
| `add_reaction` | `add_reaction(channel, message_id, reaction)` | React to a message |
| `get_reactions` | `get_reactions(channel, message_id)` | Get reactions on a message |
| `get_thread` | `get_thread(channel, parent_id)` | Read thread replies |

### Example: Wrapping ChatAdapter for an AI Agent

```rust
use chat_sdk::{ChatAdapter, ChatResult};
use chat_sdk::model::SendMessage;

/// Provide chat operations as callable tools for an AI agent.
struct ChatTools {
    adapter: Box<dyn ChatAdapter>,
}

impl ChatTools {
    /// Tool: send_message
    /// Sends a message to the specified channel.
    async fn tool_send_message(
        &self,
        channel: &str,
        text: &str,
        thread_id: Option<&str>,
    ) -> ChatResult<String> {
        let msg = SendMessage {
            channel: channel.into(),
            text: text.into(),
            thread_id: thread_id.map(|id| chat_sdk::MessageId(id.into())),
        };
        let id = self.adapter.send_message(msg).await?;
        Ok(format!("Message sent with ID: {}", id.0))
    }

    /// Tool: list_channels
    /// Lists all channels available on the platform.
    async fn tool_list_channels(&self) -> ChatResult<String> {
        let channels = self.adapter.list_channels().await?;
        let result: Vec<String> = channels
            .iter()
            .map(|ch| format!("#{} ({})", ch.name, ch.id))
            .collect();
        Ok(result.join("\n"))
    }

    /// Tool: get_messages
    /// Reads recent messages from a channel.
    async fn tool_get_messages(
        &self,
        channel: &str,
        limit: usize,
    ) -> ChatResult<String> {
        let messages = self.adapter.get_messages(channel, limit).await?;
        let result: Vec<String> = messages
            .iter()
            .map(|m| format!("[{}] {}: {}", m.timestamp, m.author.name, m.text))
            .collect();
        Ok(result.join("\n"))
    }
}
```

## Architecture

```
chat-sdk-rs/
├── crates/
│   ├── chat-sdk/              # Core library
│   │   ├── adapter.rs         # ChatAdapter trait
│   │   ├── model.rs           # Message, Channel, User, Thread, Reaction, SendMessage
│   │   ├── error.rs           # ChatError enum and ChatResult type alias
│   │   └── oauth.rs           # OAuthConfig and AuthorizationRequest
│   └── chat-sdk-cli/          # CLI binary (chat-sdk)
│       └── main.rs            # send, channels, auth subcommands
├── deny.toml                  # cargo-deny security/license policy
└── .github/workflows/ci.yml   # CI pipeline
```

## Contributing

Contributions are welcome! To get started:

```bash
git clone https://github.com/quantum-box/chat-sdk-rs.git
cd chat-sdk-rs
cargo build
cargo test
```

### Development workflow

1. **Format** - `cargo +nightly fmt`
2. **Lint** - `cargo clippy --all-targets --all-features`
3. **Test** - `cargo test --all-features`
4. **MSRV check** - Ensure compatibility with Rust 1.85.0

### CI checks

All PRs are validated by CI, which runs:

- `rustfmt` formatting check (nightly)
- `clippy` lint (all targets, all features)
- Unit tests (default features + no-default-features)
- MSRV verification (Rust 1.85.0)
- Miri memory safety testing
- `cargo-deny` license and advisory audit
- Code coverage via LLVM (uploaded to Codecov)

### Adding a new platform adapter

1. Add a feature flag in `crates/chat-sdk/Cargo.toml`
2. Implement `ChatAdapter` for the new platform
3. Add platform-specific OAuth URLs to `OAuthConfig` examples
4. Update the CLI to recognize the new platform
5. Add tests

## License

Licensed under either of

- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
