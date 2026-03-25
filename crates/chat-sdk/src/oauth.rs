use crate::error::{ChatError, ChatResult};
use oauth2::{AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, Scope};

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

impl OAuthConfig {
    /// Generate the authorization URL for the OAuth flow.
    pub fn authorize_url(&self) -> ChatResult<AuthorizationRequest> {
        let client = oauth2::basic::BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(
                AuthUrl::new(self.auth_url.clone()).map_err(|e| ChatError::OAuth(e.to_string()))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(self.redirect_url.clone())
                    .map_err(|e| ChatError::OAuth(e.to_string()))?,
            );

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
}
