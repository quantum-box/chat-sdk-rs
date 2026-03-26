//! Webhook / Event receiver infrastructure for chat-sdk.
//!
//! Provides a platform-agnostic [`WebhookHandler`] trait, concrete
//! implementations for Slack Events API and Discord Interactions, and
//! helpers to mount them in an axum HTTP server.
//!
//! # Quick start – standalone server
//!
//! ```rust,no_run
//! use chat_sdk::webhook::{WebhookConfig, start};
//!
//! # async fn run() {
//! let config = WebhookConfig {
//!     signing_secret: "slack_secret".into(),
//!     bind_address: "0.0.0.0:3000".into(),
//! };
//! let (server, mut rx) = start(config).await.unwrap();
//! while let Some(event) = rx.recv().await {
//!     println!("{event:?}");
//! }
//! # }
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::error::ChatError;

type HmacSha256 = Hmac<Sha256>;

// ===========================================================================
// WebhookHandler trait
// ===========================================================================

/// Response returned by a [`WebhookHandler`].
#[derive(Debug, Clone)]
pub struct WebhookResponse {
    pub status: StatusCode,
    pub body: String,
}

impl IntoResponse for WebhookResponse {
    fn into_response(self) -> axum::response::Response {
        (self.status, self.body).into_response()
    }
}

/// Platform-agnostic webhook/event receiver.
///
/// Implementors handle signature verification, payload parsing, and event
/// forwarding internally. The returned [`WebhookResponse`] is sent back to
/// the caller (Slack / Discord / etc.) as the HTTP response.
#[async_trait]
pub trait WebhookHandler: Send + Sync {
    /// Platform identifier (e.g. `"slack"`, `"discord"`).
    fn platform(&self) -> &str;

    /// Process an incoming webhook request.
    ///
    /// The implementation **must** verify the request signature before
    /// processing the payload.
    async fn handle_request(&self, headers: &HeaderMap, body: &[u8]) -> WebhookResponse;
}

// ===========================================================================
// Slack Event types
// ===========================================================================

/// Top-level Slack Events API payload.
///
/// Slack sends two kinds of requests:
/// 1. `url_verification` -- a challenge during endpoint registration.
/// 2. `event_callback` -- an actual event (message, reaction, etc.).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SlackEnvelope {
    #[serde(rename = "url_verification")]
    UrlVerification { challenge: String, token: String },
    #[serde(rename = "event_callback")]
    EventCallback {
        token: String,
        team_id: String,
        event: SlackEvent,
        event_id: String,
        event_time: u64,
    },
}

/// An individual Slack event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SlackEvent {
    #[serde(rename = "message")]
    Message {
        channel: String,
        user: Option<String>,
        text: Option<String>,
        ts: String,
        #[serde(default)]
        thread_ts: Option<String>,
        #[serde(default)]
        subtype: Option<String>,
    },
    #[serde(rename = "reaction_added")]
    ReactionAdded {
        user: String,
        reaction: String,
        item: ReactionItem,
        event_ts: String,
    },
    #[serde(rename = "reaction_removed")]
    ReactionRemoved {
        user: String,
        reaction: String,
        item: ReactionItem,
        event_ts: String,
    },
    #[serde(rename = "app_mention")]
    AppMention {
        channel: String,
        user: String,
        text: String,
        ts: String,
        #[serde(default)]
        thread_ts: Option<String>,
    },
}

/// Target of a reaction event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub channel: Option<String>,
    pub ts: Option<String>,
}

// ===========================================================================
// Slack signature verification
// ===========================================================================

/// Verify the Slack request signature (v0).
///
/// Slack signs every request with HMAC-SHA256 using the signing secret.
/// See <https://api.slack.com/authentication/verifying-requests-from-slack>.
pub fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &[u8],
    expected_signature: &str,
) -> Result<(), ChatError> {
    let sig_basestring = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));

    let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes())
        .map_err(|e| ChatError::Other(format!("HMAC init error: {e}")))?;
    mac.update(sig_basestring.as_bytes());
    let result = mac.finalize();
    let computed = format!("v0={}", hex::encode(result.into_bytes()));

    if !constant_time_eq(computed.as_bytes(), expected_signature.as_bytes()) {
        return Err(ChatError::Auth("invalid request signature".into()));
    }
    Ok(())
}

/// Backward-compatible alias.
pub use verify_slack_signature as verify_signature;

// ===========================================================================
// SlackWebhookHandler
// ===========================================================================

/// Slack Events API webhook handler.
///
/// Handles `url_verification` challenges and forwards `event_callback`
/// payloads to the provided channel.
pub struct SlackWebhookHandler {
    signing_secret: String,
    event_tx: mpsc::Sender<SlackEvent>,
}

impl SlackWebhookHandler {
    pub fn new(signing_secret: String, event_tx: mpsc::Sender<SlackEvent>) -> Self {
        Self {
            signing_secret,
            event_tx,
        }
    }
}

#[async_trait]
impl WebhookHandler for SlackWebhookHandler {
    fn platform(&self) -> &str {
        "slack"
    }

    async fn handle_request(&self, headers: &HeaderMap, body: &[u8]) -> WebhookResponse {
        // --- signature verification -----------------------------------------
        let timestamp = match headers
            .get("x-slack-request-timestamp")
            .and_then(|v| v.to_str().ok())
        {
            Some(ts) => ts.to_owned(),
            None => {
                warn!("missing x-slack-request-timestamp header");
                return WebhookResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: "missing timestamp header".into(),
                };
            }
        };

        let signature = match headers
            .get("x-slack-signature")
            .and_then(|v| v.to_str().ok())
        {
            Some(sig) => sig.to_owned(),
            None => {
                warn!("missing x-slack-signature header");
                return WebhookResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: "missing signature header".into(),
                };
            }
        };

        if let Err(e) = verify_slack_signature(&self.signing_secret, &timestamp, body, &signature) {
            warn!("signature verification failed: {e}");
            return WebhookResponse {
                status: StatusCode::UNAUTHORIZED,
                body: "invalid signature".into(),
            };
        }

        // --- parse envelope -------------------------------------------------
        let envelope: SlackEnvelope = match serde_json::from_slice(body) {
            Ok(env) => env,
            Err(e) => {
                error!("failed to parse Slack envelope: {e}");
                return WebhookResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: "invalid payload".into(),
                };
            }
        };

        match envelope {
            SlackEnvelope::UrlVerification { challenge, .. } => {
                debug!("responding to url_verification challenge");
                WebhookResponse {
                    status: StatusCode::OK,
                    body: challenge,
                }
            }
            SlackEnvelope::EventCallback {
                event, event_id, ..
            } => {
                debug!(event_id = %event_id, "received event callback");
                if let Err(e) = self.event_tx.send(event).await {
                    error!("failed to forward event: {e}");
                }
                WebhookResponse {
                    status: StatusCode::OK,
                    body: String::new(),
                }
            }
        }
    }
}

// ===========================================================================
// Discord Interaction types
// ===========================================================================

/// Discord interaction type constants.
pub mod discord_interaction_type {
    pub const PING: u8 = 1;
    pub const APPLICATION_COMMAND: u8 = 2;
    pub const MESSAGE_COMPONENT: u8 = 3;
    pub const APPLICATION_COMMAND_AUTOCOMPLETE: u8 = 4;
    pub const MODAL_SUBMIT: u8 = 5;
}

/// Discord interaction callback type constants.
pub mod discord_callback_type {
    pub const PONG: u8 = 1;
    pub const CHANNEL_MESSAGE_WITH_SOURCE: u8 = 4;
    pub const DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE: u8 = 5;
    pub const DEFERRED_UPDATE_MESSAGE: u8 = 6;
    pub const UPDATE_MESSAGE: u8 = 7;
}

/// A Discord interaction received via the Interactions endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInteraction {
    pub id: String,
    #[serde(rename = "type")]
    pub interaction_type: u8,
    #[serde(default)]
    pub data: Option<DiscordInteractionData>,
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub guild_id: Option<String>,
    #[serde(default)]
    pub member: Option<DiscordMember>,
    #[serde(default)]
    pub user: Option<DiscordInteractionUser>,
    pub token: String,
}

/// Data payload for application commands and components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInteractionData {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub custom_id: Option<String>,
    #[serde(default)]
    pub options: Option<Vec<DiscordCommandOption>>,
}

/// A slash-command option value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordCommandOption {
    pub name: String,
    #[serde(rename = "type")]
    pub option_type: u8,
    #[serde(default)]
    pub value: Option<serde_json::Value>,
    #[serde(default)]
    pub options: Option<Vec<DiscordCommandOption>>,
}

/// Guild member info attached to an interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordMember {
    #[serde(default)]
    pub user: Option<DiscordInteractionUser>,
    #[serde(default)]
    pub nick: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
}

/// Discord user info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInteractionUser {
    pub id: String,
    pub username: String,
    #[serde(default)]
    pub discriminator: Option<String>,
    #[serde(default)]
    pub global_name: Option<String>,
}

/// Response sent back to Discord for an interaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInteractionResponse {
    #[serde(rename = "type")]
    pub response_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiscordInteractionResponseData>,
}

/// Data payload for an interaction response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordInteractionResponseData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<u32>,
}

// ===========================================================================
// Discord signature verification
// ===========================================================================

/// Verify a Discord interaction request signature (Ed25519).
///
/// Discord signs every interaction with Ed25519 using the application's
/// public key.  The signed message is `timestamp + body`.
/// See <https://discord.com/developers/docs/interactions/overview#setting-up-an-endpoint>.
#[cfg(feature = "discord")]
pub fn verify_discord_signature(
    public_key_hex: &str,
    timestamp: &str,
    body: &[u8],
    signature_hex: &str,
) -> Result<(), ChatError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let pk_bytes: [u8; 32] = hex::decode(public_key_hex)
        .map_err(|e| ChatError::Auth(format!("invalid public key hex: {e}")))?
        .try_into()
        .map_err(|_| ChatError::Auth("public key must be 32 bytes".into()))?;

    let verifying_key = VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| ChatError::Auth(format!("invalid public key: {e}")))?;

    let sig_bytes: [u8; 64] = hex::decode(signature_hex)
        .map_err(|e| ChatError::Auth(format!("invalid signature hex: {e}")))?
        .try_into()
        .map_err(|_| ChatError::Auth("signature must be 64 bytes".into()))?;

    let signature = Signature::from_bytes(&sig_bytes);

    let mut message = Vec::with_capacity(timestamp.len() + body.len());
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body);

    verifying_key
        .verify(&message, &signature)
        .map_err(|_| ChatError::Auth("invalid request signature".into()))
}

// ===========================================================================
// DiscordWebhookHandler
// ===========================================================================

/// Discord Interactions endpoint webhook handler.
///
/// Responds to `PING` (type 1) with `PONG` automatically. All other
/// interaction types are forwarded to the provided channel and acknowledged
/// with a `DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE` (type 5) response so the
/// consumer can send a follow-up via the interaction token.
#[cfg(feature = "discord")]
pub struct DiscordWebhookHandler {
    public_key_hex: String,
    event_tx: mpsc::Sender<DiscordInteraction>,
}

#[cfg(feature = "discord")]
impl DiscordWebhookHandler {
    pub fn new(public_key_hex: String, event_tx: mpsc::Sender<DiscordInteraction>) -> Self {
        Self {
            public_key_hex,
            event_tx,
        }
    }
}

#[cfg(feature = "discord")]
#[async_trait]
impl WebhookHandler for DiscordWebhookHandler {
    fn platform(&self) -> &str {
        "discord"
    }

    async fn handle_request(&self, headers: &HeaderMap, body: &[u8]) -> WebhookResponse {
        // --- signature verification -----------------------------------------
        let timestamp = match headers
            .get("x-signature-timestamp")
            .and_then(|v| v.to_str().ok())
        {
            Some(ts) => ts.to_owned(),
            None => {
                warn!("missing x-signature-timestamp header");
                return WebhookResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: "missing timestamp header".into(),
                };
            }
        };

        let signature = match headers
            .get("x-signature-ed25519")
            .and_then(|v| v.to_str().ok())
        {
            Some(sig) => sig.to_owned(),
            None => {
                warn!("missing x-signature-ed25519 header");
                return WebhookResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: "missing signature header".into(),
                };
            }
        };

        if let Err(e) = verify_discord_signature(&self.public_key_hex, &timestamp, body, &signature)
        {
            warn!("Discord signature verification failed: {e}");
            return WebhookResponse {
                status: StatusCode::UNAUTHORIZED,
                body: "invalid signature".into(),
            };
        }

        // --- parse interaction ----------------------------------------------
        let interaction: DiscordInteraction = match serde_json::from_slice(body) {
            Ok(i) => i,
            Err(e) => {
                error!("failed to parse Discord interaction: {e}");
                return WebhookResponse {
                    status: StatusCode::BAD_REQUEST,
                    body: "invalid payload".into(),
                };
            }
        };

        // PING -> PONG
        if interaction.interaction_type == discord_interaction_type::PING {
            debug!("responding to Discord PING");
            let resp = DiscordInteractionResponse {
                response_type: discord_callback_type::PONG,
                data: None,
            };
            return WebhookResponse {
                status: StatusCode::OK,
                body: serde_json::to_string(&resp).unwrap_or_default(),
            };
        }

        // Forward all other interactions and acknowledge with deferred response
        debug!(id = %interaction.id, r#type = interaction.interaction_type, "received Discord interaction");
        if let Err(e) = self.event_tx.send(interaction).await {
            error!("failed to forward Discord interaction: {e}");
        }

        let resp = DiscordInteractionResponse {
            response_type: discord_callback_type::DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE,
            data: None,
        };
        WebhookResponse {
            status: StatusCode::OK,
            body: serde_json::to_string(&resp).unwrap_or_default(),
        }
    }
}

// ===========================================================================
// Generic axum handler
// ===========================================================================

/// Generic axum handler that delegates to a [`WebhookHandler`].
async fn webhook_handler(
    State(handler): State<Arc<dyn WebhookHandler>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    handler.handle_request(&headers, &body).await
}

/// Build a [`Router`] that serves a single [`WebhookHandler`] at `path`.
///
/// # Example
///
/// ```rust,no_run
/// use std::sync::Arc;
/// use chat_sdk::webhook::{SlackWebhookHandler, handler_router};
/// use tokio::sync::mpsc;
///
/// let (tx, _rx) = mpsc::channel(256);
/// let handler = Arc::new(SlackWebhookHandler::new("secret".into(), tx));
/// let app = handler_router("/slack/events", handler);
/// ```
pub fn handler_router(path: &str, handler: Arc<dyn WebhookHandler>) -> Router {
    Router::new()
        .route(path, post(webhook_handler))
        .with_state(handler)
}

// ===========================================================================
// Backward-compatible Slack-specific helpers
// ===========================================================================

/// Shared state for the legacy webhook handler (kept for backward compat).
#[derive(Clone)]
struct WebhookState {
    signing_secret: String,
    event_tx: mpsc::Sender<SlackEvent>,
}

/// Configuration for the webhook server.
#[derive(Debug, Clone)]
pub struct WebhookConfig {
    /// Slack signing secret used to verify request authenticity.
    pub signing_secret: String,
    /// Address to bind the HTTP server to (e.g. `"0.0.0.0:3000"`).
    pub bind_address: String,
}

/// A running webhook server handle.
///
/// Use [`start`] to create a server and obtain this handle together with
/// an event receiver channel.
pub struct WebhookServer {
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl WebhookServer {
    /// Gracefully shut down the webhook server.
    pub fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for WebhookServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Start the webhook server and return a handle + event receiver.
///
/// Events received from Slack are forwarded to the returned
/// [`mpsc::Receiver<SlackEvent>`]. The caller should spawn a task to
/// consume events from the receiver.
pub async fn start(
    config: WebhookConfig,
) -> Result<(WebhookServer, mpsc::Receiver<SlackEvent>), ChatError> {
    let (event_tx, event_rx) = mpsc::channel::<SlackEvent>(256);

    let state = Arc::new(WebhookState {
        signing_secret: config.signing_secret,
        event_tx,
    });

    let app = Router::new()
        .route("/slack/events", post(slack_events_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_address)
        .await
        .map_err(|e| ChatError::Other(format!("failed to bind {}: {e}", config.bind_address)))?;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    debug!(address = %config.bind_address, "webhook server starting");

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
    });

    Ok((
        WebhookServer {
            shutdown_tx: Some(shutdown_tx),
        },
        event_rx,
    ))
}

/// Build a webhook [`Router`] without starting a server.
///
/// Useful when you want to mount the webhook routes into an existing
/// axum application.
pub fn router(signing_secret: String, event_tx: mpsc::Sender<SlackEvent>) -> Router {
    let state = Arc::new(WebhookState {
        signing_secret,
        event_tx,
    });

    Router::new()
        .route("/slack/events", post(slack_events_handler))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Legacy axum handler (delegates to internal state)
// ---------------------------------------------------------------------------

async fn slack_events_handler(
    State(state): State<Arc<WebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let handler = SlackWebhookHandler::new(state.signing_secret.clone(), state.event_tx.clone());
    let resp = handler.handle_request(&headers, &body).await;
    (resp.status, resp.body)
}

// ===========================================================================
// Utilities
// ===========================================================================

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Slack signature verification ----------------------------------------

    #[test]
    fn verify_valid_signature() {
        let secret = "8f742231b10e8888abcd99yyyzzz85a5";
        let timestamp = "1531420618";
        let body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&api_app_id=A19GAJ72T";

        let sig_base = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(sig_base.as_bytes());
        let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        assert!(verify_slack_signature(secret, timestamp, body, &expected).is_ok());
    }

    #[test]
    fn reject_invalid_signature() {
        let secret = "my_secret";
        let timestamp = "1234567890";
        let body = b"hello";
        let bad_sig = "v0=0000000000000000000000000000000000000000000000000000000000000000";

        let result = verify_slack_signature(secret, timestamp, body, bad_sig);
        assert!(result.is_err());
    }

    #[test]
    fn constant_time_eq_same() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn constant_time_eq_different() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"short", b"longer_string"));
    }

    // -- Slack deserialization -----------------------------------------------

    #[test]
    fn parse_url_verification() {
        let json = r#"{
            "type": "url_verification",
            "token": "Jhj5dZrVaK7ZwHHjRyZWjbDl",
            "challenge": "3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P"
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::UrlVerification { challenge, .. } => {
                assert_eq!(
                    challenge,
                    "3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P"
                );
            }
            _ => panic!("expected UrlVerification"),
        }
    }

    #[test]
    fn parse_message_event() {
        let json = r#"{
            "type": "event_callback",
            "token": "XXYYZZ",
            "team_id": "T123456",
            "event": {
                "type": "message",
                "channel": "C2147483705",
                "user": "U2147483697",
                "text": "Hello world",
                "ts": "1355517523.000005",
                "thread_ts": null
            },
            "event_id": "Ev0PV52K25",
            "event_time": 1355517523
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback {
                event, event_id, ..
            } => {
                assert_eq!(event_id, "Ev0PV52K25");
                match event {
                    SlackEvent::Message { channel, text, .. } => {
                        assert_eq!(channel, "C2147483705");
                        assert_eq!(text.unwrap(), "Hello world");
                    }
                    _ => panic!("expected Message event"),
                }
            }
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn parse_reaction_added_event() {
        let json = r#"{
            "type": "event_callback",
            "token": "XXYYZZ",
            "team_id": "T123456",
            "event": {
                "type": "reaction_added",
                "user": "U123",
                "reaction": "thumbsup",
                "item": {
                    "type": "message",
                    "channel": "C0G9QF9GZ",
                    "ts": "1360782400.498405"
                },
                "event_ts": "1360782804.083113"
            },
            "event_id": "Ev0PV52K26",
            "event_time": 1360782804
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback { event, .. } => match event {
                SlackEvent::ReactionAdded {
                    reaction,
                    user,
                    item,
                    ..
                } => {
                    assert_eq!(reaction, "thumbsup");
                    assert_eq!(user, "U123");
                    assert_eq!(item.channel.unwrap(), "C0G9QF9GZ");
                }
                _ => panic!("expected ReactionAdded"),
            },
            _ => panic!("expected EventCallback"),
        }
    }

    #[test]
    fn parse_app_mention_event() {
        let json = r#"{
            "type": "event_callback",
            "token": "XXYYZZ",
            "team_id": "T123456",
            "event": {
                "type": "app_mention",
                "channel": "C0G9QF9GZ",
                "user": "U123",
                "text": "<@U456> hello bot",
                "ts": "1355517523.000005"
            },
            "event_id": "Ev0PV52K27",
            "event_time": 1355517523
        }"#;

        let envelope: SlackEnvelope = serde_json::from_str(json).unwrap();
        match envelope {
            SlackEnvelope::EventCallback { event, .. } => match event {
                SlackEvent::AppMention { text, channel, .. } => {
                    assert_eq!(text, "<@U456> hello bot");
                    assert_eq!(channel, "C0G9QF9GZ");
                }
                _ => panic!("expected AppMention"),
            },
            _ => panic!("expected EventCallback"),
        }
    }

    // -- WebhookHandler trait (Slack) ----------------------------------------

    #[tokio::test]
    async fn slack_handler_url_verification() {
        let (tx, _rx) = mpsc::channel::<SlackEvent>(16);
        let handler = SlackWebhookHandler::new("test_secret".into(), tx);

        let body = br#"{"type":"url_verification","token":"tok","challenge":"chall_abc"}"#;
        let timestamp = "1531420618";

        let sig_base = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(b"test_secret").unwrap();
        mac.update(sig_base.as_bytes());
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let mut headers = HeaderMap::new();
        headers.insert("x-slack-request-timestamp", timestamp.parse().unwrap());
        headers.insert("x-slack-signature", signature.parse().unwrap());

        let resp = handler.handle_request(&headers, body).await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_eq!(resp.body, "chall_abc");
    }

    #[tokio::test]
    async fn slack_handler_rejects_bad_signature() {
        let (tx, _rx) = mpsc::channel::<SlackEvent>(16);
        let handler = SlackWebhookHandler::new("test_secret".into(), tx);

        let mut headers = HeaderMap::new();
        headers.insert("x-slack-request-timestamp", "12345".parse().unwrap());
        headers.insert("x-slack-signature", "v0=bad".parse().unwrap());

        let resp = handler.handle_request(&headers, b"{}").await;
        assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn slack_handler_missing_headers() {
        let (tx, _rx) = mpsc::channel::<SlackEvent>(16);
        let handler = SlackWebhookHandler::new("test_secret".into(), tx);

        let resp = handler.handle_request(&HeaderMap::new(), b"{}").await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn slack_handler_forwards_event() {
        let (tx, mut rx) = mpsc::channel::<SlackEvent>(16);
        let handler = SlackWebhookHandler::new("secret".into(), tx);

        let body = br#"{"type":"event_callback","token":"t","team_id":"T1","event":{"type":"message","channel":"C1","user":"U1","text":"hi","ts":"1.0"},"event_id":"E1","event_time":1}"#;
        let timestamp = "123";

        let sig_base = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(b"secret").unwrap();
        mac.update(sig_base.as_bytes());
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let mut headers = HeaderMap::new();
        headers.insert("x-slack-request-timestamp", timestamp.parse().unwrap());
        headers.insert("x-slack-signature", signature.parse().unwrap());

        let resp = handler.handle_request(&headers, body).await;
        assert_eq!(resp.status, StatusCode::OK);

        let event = rx.recv().await.unwrap();
        match event {
            SlackEvent::Message { channel, text, .. } => {
                assert_eq!(channel, "C1");
                assert_eq!(text.unwrap(), "hi");
            }
            _ => panic!("expected Message event"),
        }
    }

    // -- Discord types -------------------------------------------------------

    #[test]
    fn parse_discord_ping() {
        let json = r#"{"id":"1","type":1,"token":"tok"}"#;
        let interaction: DiscordInteraction = serde_json::from_str(json).unwrap();
        assert_eq!(interaction.interaction_type, discord_interaction_type::PING);
        assert_eq!(interaction.id, "1");
    }

    #[test]
    fn parse_discord_application_command() {
        let json = r#"{
            "id":"12345",
            "type":2,
            "data":{"id":"cmd1","name":"hello","options":[]},
            "channel_id":"C1",
            "guild_id":"G1",
            "member":{"user":{"id":"U1","username":"alice"},"roles":[]},
            "token":"interaction_token"
        }"#;
        let interaction: DiscordInteraction = serde_json::from_str(json).unwrap();
        assert_eq!(
            interaction.interaction_type,
            discord_interaction_type::APPLICATION_COMMAND
        );
        assert_eq!(
            interaction.data.as_ref().unwrap().name.as_deref(),
            Some("hello")
        );
        assert_eq!(interaction.channel_id.as_deref(), Some("C1"));
        assert_eq!(
            interaction
                .member
                .as_ref()
                .unwrap()
                .user
                .as_ref()
                .unwrap()
                .username,
            "alice"
        );
    }

    #[test]
    fn discord_interaction_response_serialization() {
        let resp = DiscordInteractionResponse {
            response_type: discord_callback_type::PONG,
            data: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":1"));
        assert!(!json.contains("data"));

        let resp_with_data = DiscordInteractionResponse {
            response_type: discord_callback_type::CHANNEL_MESSAGE_WITH_SOURCE,
            data: Some(DiscordInteractionResponseData {
                content: Some("hello".into()),
                flags: None,
            }),
        };
        let json = serde_json::to_string(&resp_with_data).unwrap();
        assert!(json.contains("\"content\":\"hello\""));
    }

    // -- Discord signature verification --------------------------------------

    #[cfg(feature = "discord")]
    mod discord_handler_tests {
        use super::*;

        fn generate_ed25519_keypair() -> (ed25519_dalek::SigningKey, ed25519_dalek::VerifyingKey) {
            use ed25519_dalek::SigningKey;
            let signing_key = SigningKey::from_bytes(&[1u8; 32]);
            let verifying_key = signing_key.verifying_key();
            (signing_key, verifying_key)
        }

        fn sign_discord_request(
            signing_key: &ed25519_dalek::SigningKey,
            timestamp: &str,
            body: &[u8],
        ) -> String {
            use ed25519_dalek::Signer;
            let mut message = Vec::with_capacity(timestamp.len() + body.len());
            message.extend_from_slice(timestamp.as_bytes());
            message.extend_from_slice(body);
            let sig = signing_key.sign(&message);
            hex::encode(sig.to_bytes())
        }

        #[test]
        fn verify_valid_discord_signature() {
            let (signing_key, verifying_key) = generate_ed25519_keypair();
            let pk_hex = hex::encode(verifying_key.to_bytes());
            let timestamp = "1234567890";
            let body = b"test body";

            let sig_hex = sign_discord_request(&signing_key, timestamp, body);
            assert!(verify_discord_signature(&pk_hex, timestamp, body, &sig_hex).is_ok());
        }

        #[test]
        fn reject_invalid_discord_signature() {
            let (_, verifying_key) = generate_ed25519_keypair();
            let pk_hex = hex::encode(verifying_key.to_bytes());

            let result = verify_discord_signature(&pk_hex, "123", b"body", &"00".repeat(64));
            assert!(result.is_err());
        }

        #[test]
        fn reject_bad_public_key_hex() {
            let result = verify_discord_signature("not_hex", "123", b"body", &"00".repeat(64));
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn discord_handler_ping_pong() {
            let (signing_key, verifying_key) = generate_ed25519_keypair();
            let pk_hex = hex::encode(verifying_key.to_bytes());

            let (tx, _rx) = mpsc::channel::<DiscordInteraction>(16);
            let handler = DiscordWebhookHandler::new(pk_hex, tx);

            let body = br#"{"id":"1","type":1,"token":"tok"}"#;
            let timestamp = "1234567890";
            let sig_hex = sign_discord_request(&signing_key, timestamp, body);

            let mut headers = HeaderMap::new();
            headers.insert("x-signature-timestamp", timestamp.parse().unwrap());
            headers.insert("x-signature-ed25519", sig_hex.parse().unwrap());

            let resp = handler.handle_request(&headers, body).await;
            assert_eq!(resp.status, StatusCode::OK);

            let parsed: DiscordInteractionResponse = serde_json::from_str(&resp.body).unwrap();
            assert_eq!(parsed.response_type, discord_callback_type::PONG);
        }

        #[tokio::test]
        async fn discord_handler_forwards_command() {
            let (signing_key, verifying_key) = generate_ed25519_keypair();
            let pk_hex = hex::encode(verifying_key.to_bytes());

            let (tx, mut rx) = mpsc::channel::<DiscordInteraction>(16);
            let handler = DiscordWebhookHandler::new(pk_hex, tx);

            let body = br#"{"id":"99","type":2,"data":{"id":"c1","name":"test"},"token":"tok"}"#;
            let timestamp = "1234567890";
            let sig_hex = sign_discord_request(&signing_key, timestamp, body);

            let mut headers = HeaderMap::new();
            headers.insert("x-signature-timestamp", timestamp.parse().unwrap());
            headers.insert("x-signature-ed25519", sig_hex.parse().unwrap());

            let resp = handler.handle_request(&headers, body).await;
            assert_eq!(resp.status, StatusCode::OK);

            let parsed: DiscordInteractionResponse = serde_json::from_str(&resp.body).unwrap();
            assert_eq!(
                parsed.response_type,
                discord_callback_type::DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE
            );

            let interaction = rx.recv().await.unwrap();
            assert_eq!(interaction.id, "99");
            assert_eq!(
                interaction.interaction_type,
                discord_interaction_type::APPLICATION_COMMAND
            );
        }

        #[tokio::test]
        async fn discord_handler_rejects_bad_signature() {
            let (_, verifying_key) = generate_ed25519_keypair();
            let pk_hex = hex::encode(verifying_key.to_bytes());

            let (tx, _rx) = mpsc::channel::<DiscordInteraction>(16);
            let handler = DiscordWebhookHandler::new(pk_hex, tx);

            let mut headers = HeaderMap::new();
            headers.insert("x-signature-timestamp", "123".parse().unwrap());
            headers.insert("x-signature-ed25519", "00".repeat(64).parse().unwrap());

            let resp = handler.handle_request(&headers, b"{}").await;
            assert_eq!(resp.status, StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn discord_handler_missing_headers() {
            let (_, verifying_key) = generate_ed25519_keypair();
            let pk_hex = hex::encode(verifying_key.to_bytes());

            let (tx, _rx) = mpsc::channel::<DiscordInteraction>(16);
            let handler = DiscordWebhookHandler::new(pk_hex, tx);

            let resp = handler.handle_request(&HeaderMap::new(), b"{}").await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        }
    }

    // -- Integration: server + HTTP (legacy API) -----------------------------

    #[tokio::test]
    async fn webhook_url_verification_flow() {
        let signing_secret = "test_secret_123";

        let (event_tx, _event_rx) = mpsc::channel::<SlackEvent>(16);
        let state = Arc::new(WebhookState {
            signing_secret: signing_secret.to_owned(),
            event_tx,
        });

        let app = Router::new()
            .route("/slack/events", post(slack_events_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });

        let body = r#"{"type":"url_verification","token":"tok","challenge":"chall_abc"}"#;
        let timestamp = "1531420618";

        let sig_base = format!("v0:{timestamp}:{body}");
        let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes()).unwrap();
        mac.update(sig_base.as_bytes());
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/slack/events"))
            .header("x-slack-request-timestamp", timestamp)
            .header("x-slack-signature", &signature)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "chall_abc");
    }

    #[tokio::test]
    async fn webhook_rejects_bad_signature() {
        let signing_secret = "test_secret_456";

        let (event_tx, _event_rx) = mpsc::channel::<SlackEvent>(16);
        let state = Arc::new(WebhookState {
            signing_secret: signing_secret.to_owned(),
            event_tx,
        });

        let app = Router::new()
            .route("/slack/events", post(slack_events_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });

        let body = r#"{"type":"url_verification","token":"tok","challenge":"x"}"#;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/slack/events"))
            .header("x-slack-request-timestamp", "12345")
            .header("x-slack-signature", "v0=bad_signature")
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 401);
    }

    #[tokio::test]
    async fn webhook_forwards_event_to_channel() {
        let signing_secret = "test_secret_789";

        let (event_tx, mut event_rx) = mpsc::channel::<SlackEvent>(16);
        let state = Arc::new(WebhookState {
            signing_secret: signing_secret.to_owned(),
            event_tx,
        });

        let app = Router::new()
            .route("/slack/events", post(slack_events_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });

        let body = r#"{"type":"event_callback","token":"tok","team_id":"T1","event":{"type":"message","channel":"C1","user":"U1","text":"hi","ts":"1.0"},"event_id":"Ev1","event_time":1234}"#;
        let timestamp = "1531420618";

        let sig_base = format!("v0:{timestamp}:{body}");
        let mut mac = HmacSha256::new_from_slice(signing_secret.as_bytes()).unwrap();
        mac.update(sig_base.as_bytes());
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/slack/events"))
            .header("x-slack-request-timestamp", timestamp)
            .header("x-slack-signature", &signature)
            .header("content-type", "application/json")
            .body(body.to_owned())
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);

        let event = event_rx.recv().await.unwrap();
        match event {
            SlackEvent::Message { channel, text, .. } => {
                assert_eq!(channel, "C1");
                assert_eq!(text.unwrap(), "hi");
            }
            _ => panic!("expected Message event"),
        }
    }

    #[tokio::test]
    async fn webhook_missing_headers_returns_400() {
        let (event_tx, _event_rx) = mpsc::channel::<SlackEvent>(16);
        let state = Arc::new(WebhookState {
            signing_secret: "secret".to_owned(),
            event_tx,
        });

        let app = Router::new()
            .route("/slack/events", post(slack_events_handler))
            .with_state(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });

        let client = reqwest::Client::new();

        // Missing both headers
        let resp = client
            .post(format!("http://{addr}/slack/events"))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    // -- start() integration -------------------------------------------------

    #[tokio::test]
    async fn start_and_shutdown() {
        let config = WebhookConfig {
            signing_secret: "secret".to_owned(),
            bind_address: "127.0.0.1:0".to_owned(),
        };

        let (server, _rx) = start(config).await.unwrap();
        server.shutdown();
    }

    // -- handler_router integration ------------------------------------------

    #[tokio::test]
    async fn handler_router_with_slack() {
        let (tx, _rx) = mpsc::channel::<SlackEvent>(16);
        let handler: Arc<dyn WebhookHandler> =
            Arc::new(SlackWebhookHandler::new("my_secret".into(), tx));

        let app = handler_router("/events", handler);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.ok() });

        let body = br#"{"type":"url_verification","token":"t","challenge":"ok"}"#;
        let timestamp = "123";
        let sig_base = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(b"my_secret").unwrap();
        mac.update(sig_base.as_bytes());
        let signature = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://{addr}/events"))
            .header("x-slack-request-timestamp", timestamp)
            .header("x-slack-signature", &signature)
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        assert_eq!(resp.text().await.unwrap(), "ok");
    }
}
