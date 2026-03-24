use std::path::PathBuf;

use crate::error::{ChatError, ChatResult};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    RedirectUrl, Scope, StandardTokenResponse, TokenResponse, TokenUrl,
};
use serde::{Deserialize, Serialize};

type ConfiguredClient = oauth2::Client<
    oauth2::basic::BasicErrorResponse,
    StandardTokenResponse<oauth2::EmptyExtraTokenFields, oauth2::basic::BasicTokenType>,
    oauth2::StandardTokenIntrospectionResponse<
        oauth2::EmptyExtraTokenFields,
        oauth2::basic::BasicTokenType,
    >,
    oauth2::StandardRevocableToken,
    oauth2::basic::BasicRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointSet,
>;

/// OAuth configuration for a chat platform.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_url: String,
    pub scopes: Vec<String>,
}

/// Result of generating an authorization URL.
#[derive(Debug)]
pub struct AuthorizationRequest {
    pub url: String,
    pub csrf_token: String,
}

/// Token data persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scopes: Vec<String>,
    pub platform: String,
}

/// Callback query parameters received from the OAuth provider.
#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

impl OAuthConfig {
    /// Generate the authorization URL for the OAuth flow.
    pub fn authorize_url(&self) -> ChatResult<AuthorizationRequest> {
        let client = self.build_client()?;

        let mut auth_request = client.authorize_url(CsrfToken::new_random);
        for scope in &self.scopes {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
        let (url, csrf_token) = auth_request.url();

        Ok(AuthorizationRequest {
            url: url.to_string(),
            csrf_token: csrf_token.secret().clone(),
        })
    }

    /// Exchange an authorization code for an access token.
    pub async fn exchange_code(&self, code: &str) -> ChatResult<TokenData> {
        let client = self.build_client()?;

        let http_client = oauth2::reqwest::Client::builder()
            .build()
            .map_err(|e| ChatError::OAuth(format!("failed to build HTTP client: {e}")))?;

        let token_result = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&http_client)
            .await
            .map_err(|e| ChatError::OAuth(format!("token exchange failed: {e}")))?;

        let expires_at = token_result.expires_in().map(|d: std::time::Duration| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + d.as_secs()
        });

        let scopes: Vec<String> = token_result
            .scopes()
            .map(|s| s.iter().map(|sc: &oauth2::Scope| sc.to_string()).collect())
            .unwrap_or_default();

        Ok(TokenData {
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result
                .refresh_token()
                .map(|t: &oauth2::RefreshToken| t.secret().clone()),
            expires_at,
            scopes,
            platform: String::new(),
        })
    }

    /// Run an OAuth callback server on the redirect_url port, wait for the
    /// provider to redirect the user, and return the callback parameters.
    pub async fn wait_for_callback(&self, expected_csrf: &str) -> ChatResult<CallbackParams> {
        let port = extract_port(&self.redirect_url)?;
        let expected_state = expected_csrf.to_string();

        let (tx, rx) = tokio::sync::oneshot::channel::<CallbackParams>();
        let tx = std::sync::Arc::new(tokio::sync::Mutex::new(Some(tx)));

        let app = axum::Router::new().route(
            "/callback",
            axum::routing::get({
                let tx = tx.clone();
                move |axum::extract::Query(params): axum::extract::Query<CallbackParams>| {
                    let tx = tx.clone();
                    async move {
                        if let Some(sender) = tx.lock().await.take() {
                            let _ = sender.send(params);
                        }
                        axum::response::Html(
                            "<html><body><h1>Authorization successful!</h1>\
                             <p>You can close this tab.</p></body></html>",
                        )
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
            .await
            .map_err(|e| ChatError::OAuth(format!("failed to bind port {port}: {e}")))?;

        tracing::info!(port, "OAuth callback server listening");

        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        let params = rx
            .await
            .map_err(|_| ChatError::OAuth("callback channel closed".into()))?;

        if params.state != expected_state {
            return Err(ChatError::OAuth("CSRF state mismatch".into()));
        }

        Ok(params)
    }

    /// Run the full OAuth flow: generate URL, wait for callback, exchange code.
    pub async fn run_flow(&self, platform: &str) -> ChatResult<TokenData> {
        let auth = self.authorize_url()?;
        tracing::info!(url = %auth.url, "Open this URL to authorize");

        let params = self.wait_for_callback(&auth.csrf_token).await?;
        let mut token = self.exchange_code(&params.code).await?;
        token.platform = platform.to_string();
        Ok(token)
    }

    fn build_client(&self) -> ChatResult<ConfiguredClient> {
        let client = oauth2::basic::BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(
                AuthUrl::new(self.auth_url.clone()).map_err(|e| ChatError::OAuth(e.to_string()))?,
            )
            .set_token_uri(
                TokenUrl::new(self.token_url.clone())
                    .map_err(|e| ChatError::OAuth(e.to_string()))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(self.redirect_url.clone())
                    .map_err(|e| ChatError::OAuth(e.to_string()))?,
            );
        Ok(client)
    }
}

/// Token file storage.
pub struct TokenStore {
    base_dir: PathBuf,
}

impl TokenStore {
    /// Create a token store using the default config directory
    /// (`~/.config/chat-sdk/tokens/`).
    pub fn new() -> ChatResult<Self> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| ChatError::OAuth("cannot determine config directory".into()))?;
        let base_dir = config_dir.join("chat-sdk").join("tokens");
        Ok(Self { base_dir })
    }

    /// Create a token store at a specific path.
    pub fn with_dir(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Save a token to disk as `<platform>.json`.
    pub fn save(&self, token: &TokenData) -> ChatResult<()> {
        std::fs::create_dir_all(&self.base_dir)
            .map_err(|e| ChatError::OAuth(format!("failed to create token dir: {e}")))?;

        let path = self.base_dir.join(format!("{}.json", token.platform));
        let json = serde_json::to_string_pretty(token)?;
        std::fs::write(&path, json)
            .map_err(|e| ChatError::OAuth(format!("failed to write token file: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| ChatError::OAuth(format!("failed to set permissions: {e}")))?;
        }

        tracing::info!(path = %path.display(), "Token saved");
        Ok(())
    }

    /// Load a token for the given platform.
    pub fn load(&self, platform: &str) -> ChatResult<TokenData> {
        let path = self.base_dir.join(format!("{platform}.json"));
        let json = std::fs::read_to_string(&path)
            .map_err(|e| ChatError::OAuth(format!("failed to read token file: {e}")))?;
        let token: TokenData = serde_json::from_str(&json)?;
        Ok(token)
    }

    /// Check if a token exists for the given platform.
    pub fn exists(&self, platform: &str) -> bool {
        self.base_dir.join(format!("{platform}.json")).exists()
    }

    /// Delete a stored token.
    pub fn delete(&self, platform: &str) -> ChatResult<()> {
        let path = self.base_dir.join(format!("{platform}.json"));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| ChatError::OAuth(format!("failed to delete token: {e}")))?;
        }
        Ok(())
    }
}

fn extract_port(redirect_url: &str) -> ChatResult<u16> {
    let url = reqwest::Url::parse(redirect_url)
        .map_err(|e| ChatError::OAuth(format!("invalid redirect URL: {e}")))?;
    url.port()
        .ok_or_else(|| ChatError::OAuth("redirect URL must specify a port".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorize_url_generation() {
        let config = OAuthConfig {
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
            auth_url: "https://example.com/authorize".into(),
            token_url: "https://example.com/token".into(),
            redirect_url: "http://localhost:9876/callback".into(),
            scopes: vec!["read".into(), "write".into()],
        };
        let auth = config.authorize_url().unwrap();
        assert!(auth.url.starts_with("https://example.com/authorize"));
        assert!(auth.url.contains("client_id=test-client"));
        assert!(!auth.csrf_token.is_empty());
    }

    #[test]
    fn test_token_store_roundtrip() {
        let dir = std::env::temp_dir().join("chat-sdk-test-tokens");
        let _ = std::fs::remove_dir_all(&dir);
        let store = TokenStore::with_dir(dir.clone());

        let token = TokenData {
            access_token: "xoxb-test-token".into(),
            refresh_token: Some("xoxr-refresh".into()),
            expires_at: Some(1_700_000_000),
            scopes: vec!["chat:write".into()],
            platform: "slack".into(),
        };

        store.save(&token).unwrap();
        assert!(store.exists("slack"));

        let loaded = store.load("slack").unwrap();
        assert_eq!(loaded.access_token, "xoxb-test-token");
        assert_eq!(loaded.platform, "slack");

        store.delete("slack").unwrap();
        assert!(!store.exists("slack"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_port() {
        assert_eq!(
            extract_port("http://localhost:8080/callback").unwrap(),
            8080
        );
        assert!(extract_port("http://localhost/callback").is_err());
    }

    #[tokio::test]
    async fn test_callback_server_and_csrf_validation() {
        let config = OAuthConfig {
            client_id: "test".into(),
            client_secret: "secret".into(),
            auth_url: "https://example.com/authorize".into(),
            token_url: "https://example.com/token".into(),
            redirect_url: "http://localhost:19878/callback".into(),
            scopes: vec![],
        };

        let csrf = "test-csrf-state";

        let handle = tokio::spawn({
            let config = config.clone();
            let csrf = csrf.to_string();
            async move { config.wait_for_callback(&csrf).await }
        });

        let client = reqwest::Client::new();
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if client
                .get("http://localhost:19878/callback")
                .query(&[("code", "auth-code-123"), ("state", csrf)])
                .send()
                .await
                .is_ok()
            {
                break;
            }
        }

        let params = handle.await.unwrap().unwrap();
        assert_eq!(params.code, "auth-code-123");
        assert_eq!(params.state, csrf);
    }

    #[tokio::test]
    async fn test_csrf_mismatch() {
        let config = OAuthConfig {
            client_id: "test".into(),
            client_secret: "secret".into(),
            auth_url: "https://example.com/authorize".into(),
            token_url: "https://example.com/token".into(),
            redirect_url: "http://localhost:19879/callback".into(),
            scopes: vec![],
        };

        let handle = tokio::spawn({
            let config = config.clone();
            async move { config.wait_for_callback("expected-state").await }
        });

        let client = reqwest::Client::new();
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if client
                .get("http://localhost:19879/callback")
                .query(&[("code", "code"), ("state", "wrong-state")])
                .send()
                .await
                .is_ok()
            {
                break;
            }
        }

        let result = handle.await.unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("CSRF"));
    }
}
