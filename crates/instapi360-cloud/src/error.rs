//! Error and result types for the Insta360 cloud client.

use std::io;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// All failures the client can surface.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Transport-level failure (DNS, TLS, connection, timeout).
    #[error("http transport error: {0}")]
    Http(#[from] reqwest::Error),

    /// The API returned a non-zero `code`/`errCode` in its JSON envelope.
    #[error("api error {code}: {message}")]
    Api { code: i64, message: String },

    /// Authentication / session problem (missing, expired, or rejected token).
    #[error("auth error: {0}")]
    Auth(String),

    /// Request-signing failed or the signing scheme is not yet configured.
    #[error("signing error: {0}")]
    Signing(String),

    /// A response body could not be decoded into the expected shape.
    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),

    /// Local I/O failure while writing a downloaded file.
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// A download ended early: fewer bytes than the server declared.
    #[error("partial download: got {got} of {expected} bytes")]
    Partial { got: u64, expected: u64 },

    /// A downloaded part failed its MD5 integrity check.
    #[error("checksum mismatch for {file}: expected {expected}, got {got}")]
    Checksum { file: String, expected: String, got: String },

    /// A malformed or unexpected URL.
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),
}

impl Error {
    /// True when re-authenticating (refresh / re-login) is the right recovery.
    pub fn is_auth(&self) -> bool {
        matches!(self, Error::Auth(_))
    }
}
