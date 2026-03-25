use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{ChatError, ChatResult};
use crate::model::User;

// ---------------------------------------------------------------------------
// SlashCommand
// ---------------------------------------------------------------------------

/// A parsed slash command received from a chat platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashCommand {
    /// The command name without the leading `/` (e.g. `"deploy"`).
    pub name: String,
    /// Raw argument text that follows the command name.
    pub args: String,
    /// Channel where the command was invoked.
    pub channel: String,
    /// User who invoked the command.
    pub user: User,
}

impl SlashCommand {
    /// Parse a slash command from raw message text.
    ///
    /// Returns `None` when the text does not start with `/`.
    pub fn parse(text: &str, channel: String, user: User) -> Option<Self> {
        let text = text.trim();
        if !text.starts_with('/') {
            return None;
        }

        let without_slash = &text[1..];
        let (name, args) = match without_slash.split_once(char::is_whitespace) {
            Some((n, a)) => (n.to_string(), a.trim().to_string()),
            None => (without_slash.to_string(), String::new()),
        };

        if name.is_empty() {
            return None;
        }

        Some(Self {
            name,
            args,
            channel,
            user,
        })
    }
}

// ---------------------------------------------------------------------------
// CommandResponse
// ---------------------------------------------------------------------------

/// Response produced by a command handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResponse {
    /// Text body sent back to the user/channel.
    pub text: String,
    /// If `true`, only the invoking user sees the response (ephemeral).
    pub ephemeral: bool,
}

impl CommandResponse {
    /// Create a visible (in-channel) response.
    pub fn visible(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ephemeral: false,
        }
    }

    /// Create an ephemeral (private) response.
    pub fn ephemeral(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ephemeral: true,
        }
    }
}

/// Builder for constructing a [`CommandResponse`] incrementally.
pub struct ResponseBuilder {
    lines: Vec<String>,
    ephemeral: bool,
}

impl ResponseBuilder {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            ephemeral: false,
        }
    }

    /// Append a line of text.
    pub fn line(mut self, text: impl Into<String>) -> Self {
        self.lines.push(text.into());
        self
    }

    /// Mark the response as ephemeral.
    pub fn ephemeral(mut self) -> Self {
        self.ephemeral = true;
        self
    }

    /// Build the final [`CommandResponse`].
    pub fn build(self) -> CommandResponse {
        CommandResponse {
            text: self.lines.join("\n"),
            ephemeral: self.ephemeral,
        }
    }
}

impl Default for ResponseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Handler type
// ---------------------------------------------------------------------------

/// A boxed, async command handler function.
pub type HandlerFn = Arc<
    dyn Fn(SlashCommand) -> Pin<Box<dyn Future<Output = ChatResult<CommandResponse>> + Send>>
        + Send
        + Sync,
>;

// ---------------------------------------------------------------------------
// CommandRouter
// ---------------------------------------------------------------------------

/// Routes incoming slash commands to registered handlers.
pub struct CommandRouter {
    handlers: HashMap<String, HandlerFn>,
    fallback: Option<HandlerFn>,
}

impl CommandRouter {
    /// Create an empty router.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            fallback: None,
        }
    }

    /// Register a handler for a command name (without leading `/`).
    pub fn command<F, Fut>(mut self, name: impl Into<String>, handler: F) -> Self
    where
        F: Fn(SlashCommand) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ChatResult<CommandResponse>> + Send + 'static,
    {
        let handler = Arc::new(move |cmd: SlashCommand| {
            Box::pin(handler(cmd))
                as Pin<Box<dyn Future<Output = ChatResult<CommandResponse>> + Send>>
        });
        self.handlers.insert(name.into(), handler);
        self
    }

    /// Register a fallback handler for unrecognised commands.
    pub fn fallback<F, Fut>(mut self, handler: F) -> Self
    where
        F: Fn(SlashCommand) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ChatResult<CommandResponse>> + Send + 'static,
    {
        self.fallback = Some(Arc::new(move |cmd: SlashCommand| {
            Box::pin(handler(cmd))
                as Pin<Box<dyn Future<Output = ChatResult<CommandResponse>> + Send>>
        }));
        self
    }

    /// Dispatch a [`SlashCommand`] to the matching handler.
    pub async fn dispatch(&self, cmd: SlashCommand) -> ChatResult<CommandResponse> {
        if let Some(handler) = self.handlers.get(&cmd.name) {
            handler(cmd).await
        } else if let Some(ref fallback) = self.fallback {
            fallback(cmd).await
        } else {
            Err(ChatError::Other(format!("unknown command: /{}", cmd.name)))
        }
    }

    /// Returns the names of all registered commands.
    pub fn commands(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    /// Returns `true` if a handler is registered for the given command name.
    pub fn has_command(&self, name: &str) -> bool {
        self.handlers.contains_key(name)
    }
}

impl Default for CommandRouter {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_user() -> User {
        User {
            id: "U123".into(),
            name: "alice".into(),
            display_name: None,
        }
    }

    // -- SlashCommand::parse --------------------------------------------------

    #[test]
    fn parse_simple_command() {
        let cmd = SlashCommand::parse("/deploy", "general".into(), test_user()).unwrap();
        assert_eq!(cmd.name, "deploy");
        assert_eq!(cmd.args, "");
        assert_eq!(cmd.channel, "general");
    }

    #[test]
    fn parse_command_with_args() {
        let cmd =
            SlashCommand::parse("/deploy production --force", "ops".into(), test_user()).unwrap();
        assert_eq!(cmd.name, "deploy");
        assert_eq!(cmd.args, "production --force");
    }

    #[test]
    fn parse_ignores_non_command() {
        assert!(SlashCommand::parse("hello world", "ch".into(), test_user()).is_none());
    }

    #[test]
    fn parse_ignores_empty_slash() {
        assert!(SlashCommand::parse("/", "ch".into(), test_user()).is_none());
    }

    #[test]
    fn parse_trims_whitespace() {
        let cmd = SlashCommand::parse("  /status  ", "ch".into(), test_user()).unwrap();
        assert_eq!(cmd.name, "status");
        assert_eq!(cmd.args, "");
    }

    #[test]
    fn parse_preserves_user() {
        let user = User {
            id: "U999".into(),
            name: "bob".into(),
            display_name: Some("Bobby".into()),
        };
        let cmd = SlashCommand::parse("/ping", "ch".into(), user).unwrap();
        assert_eq!(cmd.user.id, "U999");
        assert_eq!(cmd.user.display_name.as_deref(), Some("Bobby"));
    }

    // -- CommandResponse & ResponseBuilder ------------------------------------

    #[test]
    fn visible_response() {
        let r = CommandResponse::visible("ok");
        assert_eq!(r.text, "ok");
        assert!(!r.ephemeral);
    }

    #[test]
    fn ephemeral_response() {
        let r = CommandResponse::ephemeral("secret");
        assert_eq!(r.text, "secret");
        assert!(r.ephemeral);
    }

    #[test]
    fn response_builder() {
        let r = ResponseBuilder::new()
            .line("line1")
            .line("line2")
            .ephemeral()
            .build();
        assert_eq!(r.text, "line1\nline2");
        assert!(r.ephemeral);
    }

    #[test]
    fn response_builder_default() {
        let r = ResponseBuilder::default().line("hi").build();
        assert_eq!(r.text, "hi");
        assert!(!r.ephemeral);
    }

    // -- CommandRouter --------------------------------------------------------

    #[tokio::test]
    async fn dispatch_registered_command() {
        let router = CommandRouter::new().command("ping", |_cmd| async {
            Ok(CommandResponse::visible("pong"))
        });

        let cmd = SlashCommand::parse("/ping", "ch".into(), test_user()).unwrap();
        let resp = router.dispatch(cmd).await.unwrap();
        assert_eq!(resp.text, "pong");
    }

    #[tokio::test]
    async fn dispatch_with_args() {
        let router = CommandRouter::new().command("echo", |cmd| async move {
            Ok(CommandResponse::visible(cmd.args.clone()))
        });

        let cmd = SlashCommand::parse("/echo hello world", "ch".into(), test_user()).unwrap();
        let resp = router.dispatch(cmd).await.unwrap();
        assert_eq!(resp.text, "hello world");
    }

    #[tokio::test]
    async fn dispatch_unknown_returns_error() {
        let router = CommandRouter::new();
        let cmd = SlashCommand::parse("/nope", "ch".into(), test_user()).unwrap();
        let err = router.dispatch(cmd).await.unwrap_err();
        assert!(err.to_string().contains("unknown command: /nope"));
    }

    #[tokio::test]
    async fn dispatch_fallback() {
        let router = CommandRouter::new().fallback(|cmd| async move {
            Ok(CommandResponse::ephemeral(format!(
                "unknown: /{}",
                cmd.name
            )))
        });

        let cmd = SlashCommand::parse("/xyz", "ch".into(), test_user()).unwrap();
        let resp = router.dispatch(cmd).await.unwrap();
        assert_eq!(resp.text, "unknown: /xyz");
        assert!(resp.ephemeral);
    }

    #[tokio::test]
    async fn dispatch_prefers_exact_over_fallback() {
        let router = CommandRouter::new()
            .command("ping", |_| async { Ok(CommandResponse::visible("pong")) })
            .fallback(|_| async { Ok(CommandResponse::visible("fallback")) });

        let cmd = SlashCommand::parse("/ping", "ch".into(), test_user()).unwrap();
        let resp = router.dispatch(cmd).await.unwrap();
        assert_eq!(resp.text, "pong");
    }

    #[test]
    fn commands_list() {
        let router = CommandRouter::new()
            .command("a", |_| async { Ok(CommandResponse::visible("")) })
            .command("b", |_| async { Ok(CommandResponse::visible("")) });

        let mut names = router.commands();
        names.sort();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn has_command() {
        let router =
            CommandRouter::new().command("ping", |_| async { Ok(CommandResponse::visible("")) });
        assert!(router.has_command("ping"));
        assert!(!router.has_command("pong"));
    }

    #[tokio::test]
    async fn multiple_commands() {
        let router = CommandRouter::new()
            .command("ping", |_| async { Ok(CommandResponse::visible("pong")) })
            .command("help", |_| async {
                Ok(CommandResponse::visible("available: /ping, /help"))
            });

        let c1 = SlashCommand::parse("/ping", "ch".into(), test_user()).unwrap();
        assert_eq!(router.dispatch(c1).await.unwrap().text, "pong");

        let c2 = SlashCommand::parse("/help", "ch".into(), test_user()).unwrap();
        assert_eq!(
            router.dispatch(c2).await.unwrap().text,
            "available: /ping, /help"
        );
    }
}
