use std::sync::Arc;

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

// ---------------------------------------------------------------------------
// Slack Event types
// ---------------------------------------------------------------------------

/// Top-level Slack Events API payload.
///
/// Slack sends two kinds of requests:
/// 1. `url_verification` – a challenge during endpoint registration.
/// 2. `event_callback` – an actual event (message, reaction, etc.).
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

// ---------------------------------------------------------------------------
// Signature verification
// ---------------------------------------------------------------------------

/// Verify the Slack request signature (v0).
///
/// Slack signs every request with HMAC-SHA256 using the signing secret.
/// See <https://api.slack.com/authentication/verifying-requests-from-slack>.
pub fn verify_signature(
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

// ---------------------------------------------------------------------------
// Webhook server
// ---------------------------------------------------------------------------

/// Shared state for the webhook handler.
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
// Axum handler
// ---------------------------------------------------------------------------

async fn slack_events_handler(
    State(state): State<Arc<WebhookState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // --- signature verification -------------------------------------------
    let timestamp = match headers
        .get("x-slack-request-timestamp")
        .and_then(|v| v.to_str().ok())
    {
        Some(ts) => ts.to_owned(),
        None => {
            warn!("missing x-slack-request-timestamp header");
            return (
                StatusCode::BAD_REQUEST,
                "missing timestamp header".to_owned(),
            );
        }
    };

    let signature = match headers
        .get("x-slack-signature")
        .and_then(|v| v.to_str().ok())
    {
        Some(sig) => sig.to_owned(),
        None => {
            warn!("missing x-slack-signature header");
            return (
                StatusCode::BAD_REQUEST,
                "missing signature header".to_owned(),
            );
        }
    };

    if let Err(e) = verify_signature(&state.signing_secret, &timestamp, &body, &signature) {
        warn!("signature verification failed: {e}");
        return (StatusCode::UNAUTHORIZED, "invalid signature".to_owned());
    }

    // --- parse envelope ---------------------------------------------------
    let envelope: SlackEnvelope = match serde_json::from_slice(&body) {
        Ok(env) => env,
        Err(e) => {
            error!("failed to parse Slack envelope: {e}");
            return (StatusCode::BAD_REQUEST, "invalid payload".to_owned());
        }
    };

    match envelope {
        SlackEnvelope::UrlVerification { challenge, .. } => {
            debug!("responding to url_verification challenge");
            (StatusCode::OK, challenge)
        }
        SlackEnvelope::EventCallback {
            event, event_id, ..
        } => {
            debug!(event_id = %event_id, "received event callback");
            if let Err(e) = state.event_tx.send(event).await {
                error!("failed to forward event: {e}");
            }
            (StatusCode::OK, String::new())
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- signature verification -------------------------------------------

    #[test]
    fn verify_valid_signature() {
        let secret = "8f742231b10e8888abcd99yyyzzz85a5";
        let timestamp = "1531420618";
        let body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&api_app_id=A19GAJ72T";

        let sig_base = format!("v0:{timestamp}:{}", String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(sig_base.as_bytes());
        let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        assert!(verify_signature(secret, timestamp, body, &expected).is_ok());
    }

    #[test]
    fn reject_invalid_signature() {
        let secret = "my_secret";
        let timestamp = "1234567890";
        let body = b"hello";
        let bad_sig = "v0=0000000000000000000000000000000000000000000000000000000000000000";

        let result = verify_signature(secret, timestamp, body, bad_sig);
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

    // -- deserialization --------------------------------------------------

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

    // -- integration: server + HTTP ----------------------------------------

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

    // -- start() integration ----------------------------------------------

    #[tokio::test]
    async fn start_and_shutdown() {
        let config = WebhookConfig {
            signing_secret: "secret".to_owned(),
            bind_address: "127.0.0.1:0".to_owned(),
        };

        let (server, _rx) = start(config).await.unwrap();
        server.shutdown();
    }
}
