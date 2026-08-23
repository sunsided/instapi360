//! Session / token state and the pluggable persistence trait.
//!
//! A [`Session`] holds the tokens returned by login. Persistence is delegated
//! to a [`SessionStore`] the caller provides — the CLI uses a JSON file (and
//! optionally the OS keyring); a future Android build uses
//! EncryptedSharedPreferences. Keeping this behind a trait keeps the core crate
//! free of any filesystem assumption.

use serde::{Deserialize, Serialize};

/// The bearer material for authenticated requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Sent as `X-User-Token`.
    pub user_token: String,
    /// Used with `/account/v2/refreshToken` to mint a fresh `user_token`.
    pub refresh_token: Option<String>,
    /// Unix seconds at which `user_token` expires, if known.
    pub expires_at: Option<i64>,
    /// The account id this session belongs to (for cache/scoping).
    pub user_id: Option<String>,
}

impl Session {
    /// Build a session from just a captured `X-User-Token` (auth strategy 5c).
    pub fn from_token(user_token: impl Into<String>) -> Self {
        Session {
            user_token: user_token.into(),
            refresh_token: None,
            expires_at: None,
            user_id: None,
        }
    }

    /// True when the token is known to be expired at `now_unix`.
    pub fn is_expired(&self, now_unix: i64) -> bool {
        self.expires_at.map(|e| now_unix >= e).unwrap_or(false)
    }
}

/// Persistence for a [`Session`]. Implementations decide *where* (file, keyring,
/// Android secure storage). All methods are synchronous and cheap.
pub trait SessionStore: Send + Sync {
    fn load(&self) -> crate::Result<Option<Session>>;
    fn save(&self, session: &Session) -> crate::Result<()>;
    fn clear(&self) -> crate::Result<()>;
}
