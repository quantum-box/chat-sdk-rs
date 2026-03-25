//! State adapter trait and implementations for thread subscription and session management.
//!
//! Provides a [`StateAdapter`] trait that abstracts persistent state operations,
//! with an [`InMemoryStateAdapter`] for development/testing and an optional
//! [`RedisStateAdapter`] (behind the `redis` feature flag) for production use.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::error::ChatResult;

/// A user session with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session identifier.
    pub id: String,
    /// Associated user identifier.
    pub user_id: String,
    /// Platform name (e.g. "slack", "discord").
    pub platform: String,
    /// Arbitrary key-value metadata.
    pub metadata: HashMap<String, String>,
    /// When the session was created (seconds since UNIX epoch).
    pub created_at: u64,
    /// When the session was last updated (seconds since UNIX epoch).
    pub updated_at: u64,
    /// Optional expiration time (seconds since UNIX epoch).
    pub expires_at: Option<u64>,
}

impl Session {
    /// Create a new session with the given id, user, and platform.
    pub fn new(
        id: impl Into<String>,
        user_id: impl Into<String>,
        platform: impl Into<String>,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        Self {
            id: id.into(),
            user_id: user_id.into(),
            platform: platform.into(),
            metadata: HashMap::new(),
            created_at: now,
            updated_at: now,
            expires_at: None,
        }
    }

    /// Set the TTL (time-to-live) for this session.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        self.expires_at = Some(now + ttl.as_secs());
        self
    }

    /// Insert a metadata key-value pair.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Returns true if the session has expired.
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            now >= expires_at
        } else {
            false
        }
    }
}

/// Thread subscription key: (channel, thread_id).
type ThreadKey = (String, String);

/// Trait for managing thread subscriptions and user sessions.
///
/// Implementations must be safe to share across async tasks (`Send + Sync`).
#[async_trait]
pub trait StateAdapter: Send + Sync {
    // ── Thread subscriptions ──

    /// Subscribe a user to a thread in a channel.
    async fn subscribe(&self, channel: &str, thread_id: &str, user_id: &str) -> ChatResult<()>;

    /// Unsubscribe a user from a thread.
    async fn unsubscribe(&self, channel: &str, thread_id: &str, user_id: &str) -> ChatResult<()>;

    /// Get all subscribers for a thread.
    async fn get_subscribers(&self, channel: &str, thread_id: &str) -> ChatResult<Vec<String>>;

    /// Check whether a user is subscribed to a thread.
    async fn is_subscribed(
        &self,
        channel: &str,
        thread_id: &str,
        user_id: &str,
    ) -> ChatResult<bool>;

    // ── Session management ──

    /// Store (create or update) a session.
    async fn set_session(&self, session: Session) -> ChatResult<()>;

    /// Retrieve a session by its ID. Returns `None` if not found or expired.
    async fn get_session(&self, session_id: &str) -> ChatResult<Option<Session>>;

    /// Delete a session by its ID.
    async fn delete_session(&self, session_id: &str) -> ChatResult<()>;

    /// List all active (non-expired) sessions for a given user.
    async fn list_user_sessions(&self, user_id: &str) -> ChatResult<Vec<Session>>;
}

// ── In-memory implementation ──

/// In-memory state adapter for development and testing.
///
/// All data lives in process memory and is lost on restart.
/// Thread-safe via `Arc<RwLock<...>>`.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStateAdapter {
    subscriptions: Arc<RwLock<HashMap<ThreadKey, HashSet<String>>>>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl InMemoryStateAdapter {
    /// Create a new empty in-memory state adapter.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl StateAdapter for InMemoryStateAdapter {
    async fn subscribe(&self, channel: &str, thread_id: &str, user_id: &str) -> ChatResult<()> {
        let key = (channel.to_owned(), thread_id.to_owned());
        let mut subs = self.subscriptions.write().await;
        subs.entry(key).or_default().insert(user_id.to_owned());
        Ok(())
    }

    async fn unsubscribe(&self, channel: &str, thread_id: &str, user_id: &str) -> ChatResult<()> {
        let key = (channel.to_owned(), thread_id.to_owned());
        let mut subs = self.subscriptions.write().await;
        if let Some(set) = subs.get_mut(&key) {
            set.remove(user_id);
            if set.is_empty() {
                subs.remove(&key);
            }
        }
        Ok(())
    }

    async fn get_subscribers(&self, channel: &str, thread_id: &str) -> ChatResult<Vec<String>> {
        let key = (channel.to_owned(), thread_id.to_owned());
        let subs = self.subscriptions.read().await;
        Ok(subs
            .get(&key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default())
    }

    async fn is_subscribed(
        &self,
        channel: &str,
        thread_id: &str,
        user_id: &str,
    ) -> ChatResult<bool> {
        let key = (channel.to_owned(), thread_id.to_owned());
        let subs = self.subscriptions.read().await;
        Ok(subs.get(&key).is_some_and(|set| set.contains(user_id)))
    }

    async fn set_session(&self, session: Session) -> ChatResult<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session);
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> ChatResult<Option<Session>> {
        let mut sessions = self.sessions.write().await;
        match sessions.get(session_id) {
            Some(session) if session.is_expired() => {
                sessions.remove(session_id);
                Ok(None)
            }
            Some(session) => Ok(Some(session.clone())),
            None => Ok(None),
        }
    }

    async fn delete_session(&self, session_id: &str) -> ChatResult<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }

    async fn list_user_sessions(&self, user_id: &str) -> ChatResult<Vec<Session>> {
        let mut sessions = self.sessions.write().await;
        let mut expired_ids = Vec::new();
        let mut result = Vec::new();

        for (id, session) in sessions.iter() {
            if session.is_expired() {
                expired_ids.push(id.clone());
            } else if session.user_id == user_id {
                result.push(session.clone());
            }
        }

        for id in expired_ids {
            sessions.remove(&id);
        }

        Ok(result)
    }
}

// ── Redis implementation ──

#[cfg(feature = "redis")]
mod redis_impl {
    use super::*;
    use crate::error::ChatError;

    /// Redis-backed state adapter for production use.
    ///
    /// Requires the `redis` feature flag. Uses Redis sets for thread subscriptions
    /// and Redis hashes for session storage with optional TTL support.
    #[derive(Clone)]
    pub struct RedisStateAdapter {
        client: redis::Client,
        prefix: String,
    }

    impl RedisStateAdapter {
        /// Connect to Redis with the given URL (e.g. `redis://127.0.0.1/`).
        pub fn new(url: &str) -> ChatResult<Self> {
            let client = redis::Client::open(url).map_err(|e| ChatError::Other(e.to_string()))?;
            Ok(Self {
                client,
                prefix: "chat-sdk".to_owned(),
            })
        }

        /// Set a custom key prefix (default: `"chat-sdk"`).
        pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
            self.prefix = prefix.into();
            self
        }

        fn sub_key(&self, channel: &str, thread_id: &str) -> String {
            format!("{}:sub:{}:{}", self.prefix, channel, thread_id)
        }

        fn session_key(&self, session_id: &str) -> String {
            format!("{}:session:{}", self.prefix, session_id)
        }

        fn user_sessions_key(&self, user_id: &str) -> String {
            format!("{}:user-sessions:{}", self.prefix, user_id)
        }

        async fn get_conn(&self) -> ChatResult<redis::aio::MultiplexedConnection> {
            self.client
                .get_multiplexed_async_connection()
                .await
                .map_err(|e| ChatError::Other(format!("Redis connection error: {e}")))
        }
    }

    #[async_trait]
    impl StateAdapter for RedisStateAdapter {
        async fn subscribe(&self, channel: &str, thread_id: &str, user_id: &str) -> ChatResult<()> {
            let mut conn = self.get_conn().await?;
            let key = self.sub_key(channel, thread_id);
            redis::cmd("SADD")
                .arg(&key)
                .arg(user_id)
                .exec_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;
            Ok(())
        }

        async fn unsubscribe(
            &self,
            channel: &str,
            thread_id: &str,
            user_id: &str,
        ) -> ChatResult<()> {
            let mut conn = self.get_conn().await?;
            let key = self.sub_key(channel, thread_id);
            redis::cmd("SREM")
                .arg(&key)
                .arg(user_id)
                .exec_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;
            Ok(())
        }

        async fn get_subscribers(&self, channel: &str, thread_id: &str) -> ChatResult<Vec<String>> {
            let mut conn = self.get_conn().await?;
            let key = self.sub_key(channel, thread_id);
            let members: Vec<String> = redis::cmd("SMEMBERS")
                .arg(&key)
                .query_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;
            Ok(members)
        }

        async fn is_subscribed(
            &self,
            channel: &str,
            thread_id: &str,
            user_id: &str,
        ) -> ChatResult<bool> {
            let mut conn = self.get_conn().await?;
            let key = self.sub_key(channel, thread_id);
            let result: bool = redis::cmd("SISMEMBER")
                .arg(&key)
                .arg(user_id)
                .query_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;
            Ok(result)
        }

        async fn set_session(&self, session: Session) -> ChatResult<()> {
            let mut conn = self.get_conn().await?;
            let key = self.session_key(&session.id);
            let user_sessions_key = self.user_sessions_key(&session.user_id);
            let json =
                serde_json::to_string(&session).map_err(|e| ChatError::Other(e.to_string()))?;

            // Store session data.
            redis::cmd("SET")
                .arg(&key)
                .arg(&json)
                .exec_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;

            // Set TTL if session has an expiration.
            if let Some(expires_at) = session.expires_at {
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_secs();
                if expires_at > now {
                    let ttl = expires_at - now;
                    redis::cmd("EXPIRE")
                        .arg(&key)
                        .arg(ttl)
                        .exec_async(&mut conn)
                        .await
                        .map_err(|e| ChatError::Other(e.to_string()))?;
                }
            }

            // Track session in user's session set.
            redis::cmd("SADD")
                .arg(&user_sessions_key)
                .arg(&session.id)
                .exec_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;

            Ok(())
        }

        async fn get_session(&self, session_id: &str) -> ChatResult<Option<Session>> {
            let mut conn = self.get_conn().await?;
            let key = self.session_key(session_id);
            let result: Option<String> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;

            match result {
                Some(json) => {
                    let session: Session =
                        serde_json::from_str(&json).map_err(|e| ChatError::Other(e.to_string()))?;
                    Ok(Some(session))
                }
                None => Ok(None),
            }
        }

        async fn delete_session(&self, session_id: &str) -> ChatResult<()> {
            let mut conn = self.get_conn().await?;
            let key = self.session_key(session_id);

            // Retrieve session to clean up user index.
            let existing: Option<String> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;
            if let Some(session) =
                existing.and_then(|json| serde_json::from_str::<Session>(&json).ok())
            {
                let user_key = self.user_sessions_key(&session.user_id);
                redis::cmd("SREM")
                    .arg(&user_key)
                    .arg(session_id)
                    .exec_async(&mut conn)
                    .await
                    .map_err(|e| ChatError::Other(e.to_string()))?;
            }

            redis::cmd("DEL")
                .arg(&key)
                .exec_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;
            Ok(())
        }

        async fn list_user_sessions(&self, user_id: &str) -> ChatResult<Vec<Session>> {
            let mut conn = self.get_conn().await?;
            let user_sessions_key = self.user_sessions_key(user_id);

            let session_ids: Vec<String> = redis::cmd("SMEMBERS")
                .arg(&user_sessions_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| ChatError::Other(e.to_string()))?;

            let mut sessions = Vec::new();
            let mut stale_ids = Vec::new();

            for id in &session_ids {
                match self.get_session(id).await? {
                    Some(session) => sessions.push(session),
                    None => stale_ids.push(id.clone()),
                }
            }

            // Clean up stale references.
            for id in &stale_ids {
                redis::cmd("SREM")
                    .arg(&user_sessions_key)
                    .arg(id)
                    .exec_async(&mut conn)
                    .await
                    .map_err(|e| ChatError::Other(e.to_string()))?;
            }

            Ok(sessions)
        }
    }

    impl std::fmt::Debug for RedisStateAdapter {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RedisStateAdapter")
                .field("prefix", &self.prefix)
                .finish_non_exhaustive()
        }
    }
}

#[cfg(feature = "redis")]
pub use redis_impl::RedisStateAdapter;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_and_get_subscribers() {
        let state = InMemoryStateAdapter::new();
        state.subscribe("C1", "T1", "U1").await.unwrap();
        state.subscribe("C1", "T1", "U2").await.unwrap();

        let mut subs = state.get_subscribers("C1", "T1").await.unwrap();
        subs.sort();
        assert_eq!(subs, vec!["U1", "U2"]);
    }

    #[tokio::test]
    async fn subscribe_is_idempotent() {
        let state = InMemoryStateAdapter::new();
        state.subscribe("C1", "T1", "U1").await.unwrap();
        state.subscribe("C1", "T1", "U1").await.unwrap();

        let subs = state.get_subscribers("C1", "T1").await.unwrap();
        assert_eq!(subs.len(), 1);
    }

    #[tokio::test]
    async fn unsubscribe_removes_user() {
        let state = InMemoryStateAdapter::new();
        state.subscribe("C1", "T1", "U1").await.unwrap();
        state.subscribe("C1", "T1", "U2").await.unwrap();
        state.unsubscribe("C1", "T1", "U1").await.unwrap();

        let subs = state.get_subscribers("C1", "T1").await.unwrap();
        assert_eq!(subs, vec!["U2"]);
    }

    #[tokio::test]
    async fn unsubscribe_nonexistent_is_ok() {
        let state = InMemoryStateAdapter::new();
        assert!(state.unsubscribe("C1", "T1", "U1").await.is_ok());
    }

    #[tokio::test]
    async fn is_subscribed_check() {
        let state = InMemoryStateAdapter::new();
        state.subscribe("C1", "T1", "U1").await.unwrap();

        assert!(state.is_subscribed("C1", "T1", "U1").await.unwrap());
        assert!(!state.is_subscribed("C1", "T1", "U2").await.unwrap());
    }

    #[tokio::test]
    async fn get_subscribers_empty_thread() {
        let state = InMemoryStateAdapter::new();
        let subs = state.get_subscribers("C1", "T_NONE").await.unwrap();
        assert!(subs.is_empty());
    }

    #[tokio::test]
    async fn different_threads_are_independent() {
        let state = InMemoryStateAdapter::new();
        state.subscribe("C1", "T1", "U1").await.unwrap();
        state.subscribe("C1", "T2", "U2").await.unwrap();

        assert!(state.is_subscribed("C1", "T1", "U1").await.unwrap());
        assert!(!state.is_subscribed("C1", "T1", "U2").await.unwrap());
        assert!(state.is_subscribed("C1", "T2", "U2").await.unwrap());
    }

    #[tokio::test]
    async fn session_set_and_get() {
        let state = InMemoryStateAdapter::new();
        let session = Session::new("s1", "U1", "slack").with_metadata("team", "engineering");

        state.set_session(session).await.unwrap();

        let retrieved = state.get_session("s1").await.unwrap().unwrap();
        assert_eq!(retrieved.id, "s1");
        assert_eq!(retrieved.user_id, "U1");
        assert_eq!(retrieved.platform, "slack");
        assert_eq!(retrieved.metadata.get("team").unwrap(), "engineering");
    }

    #[tokio::test]
    async fn session_get_nonexistent() {
        let state = InMemoryStateAdapter::new();
        assert!(state.get_session("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn session_delete() {
        let state = InMemoryStateAdapter::new();
        state
            .set_session(Session::new("s1", "U1", "slack"))
            .await
            .unwrap();
        state.delete_session("s1").await.unwrap();
        assert!(state.get_session("s1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn session_delete_nonexistent_is_ok() {
        let state = InMemoryStateAdapter::new();
        assert!(state.delete_session("nope").await.is_ok());
    }

    #[tokio::test]
    async fn list_user_sessions_filters_by_user() {
        let state = InMemoryStateAdapter::new();
        state
            .set_session(Session::new("s1", "U1", "slack"))
            .await
            .unwrap();
        state
            .set_session(Session::new("s2", "U1", "discord"))
            .await
            .unwrap();
        state
            .set_session(Session::new("s3", "U2", "slack"))
            .await
            .unwrap();

        let sessions = state.list_user_sessions("U1").await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().all(|s| s.user_id == "U1"));
    }

    #[tokio::test]
    async fn expired_session_is_not_returned() {
        let state = InMemoryStateAdapter::new();
        let mut session = Session::new("s1", "U1", "slack");
        // Set expiration in the past.
        session.expires_at = Some(0);

        state.set_session(session).await.unwrap();
        assert!(state.get_session("s1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn expired_sessions_cleaned_from_user_list() {
        let state = InMemoryStateAdapter::new();

        let mut expired = Session::new("s1", "U1", "slack");
        expired.expires_at = Some(0);
        state.set_session(expired).await.unwrap();

        state
            .set_session(Session::new("s2", "U1", "discord"))
            .await
            .unwrap();

        let sessions = state.list_user_sessions("U1").await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s2");
    }

    #[tokio::test]
    async fn session_update_overwrites() {
        let state = InMemoryStateAdapter::new();
        state
            .set_session(Session::new("s1", "U1", "slack").with_metadata("v", "1"))
            .await
            .unwrap();
        state
            .set_session(Session::new("s1", "U1", "slack").with_metadata("v", "2"))
            .await
            .unwrap();

        let session = state.get_session("s1").await.unwrap().unwrap();
        assert_eq!(session.metadata.get("v").unwrap(), "2");
    }

    #[tokio::test]
    async fn session_with_ttl() {
        let session = Session::new("s1", "U1", "slack").with_ttl(Duration::from_secs(3600));
        assert!(session.expires_at.is_some());
        assert!(!session.is_expired());
    }

    #[tokio::test]
    async fn clone_in_memory_shares_state() {
        let state = InMemoryStateAdapter::new();
        let state2 = state.clone();

        state.subscribe("C1", "T1", "U1").await.unwrap();
        assert!(state2.is_subscribed("C1", "T1", "U1").await.unwrap());
    }
}
