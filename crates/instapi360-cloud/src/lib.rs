//! # instapi360-cloud
//!
//! Async, platform-agnostic client for the Insta360 cloud: authenticate, list
//! cloud media, and download **original** camera files (Ace Pro 2 `.insv`/
//! `.mp4`, X5 dual-lens `.insv`) for local editing.
//!
//! Design constraints (this crate is meant to cross-compile to Android):
//! - TLS via `rustls` only (no OpenSSL/native-tls in the dependency graph).
//! - No filesystem assumptions in the core: downloads write to a caller-owned
//!   `AsyncWrite`; session persistence is a caller-provided [`SessionStore`].
//!
//! ## Status
//! Authentication is the session token alone — no request signing for
//! list/download. Import a token once and the session renews itself via
//! [`Client::refresh`] (the API mints a fresh token from the current one, so no
//! separate refresh token is needed). Credential login is reCAPTCHA-gated and
//! not implemented here.
//!
//! ## Quick start
//! ```no_run
//! # async fn run() -> instapi360_cloud::Result<()> {
//! use instapi360_cloud::{Client, ClientConfig, Region, Session, PageCursor};
//! let client = Client::new(ClientConfig::windows(Region::Global))?;
//! let session = Session::from_token("<captured X-User-Token>");
//! let page = client.list_media(&session, PageCursor::first(50)).await?;
//! for m in &page.items {
//!     println!("{} ({} bytes, {} parts)", m.name, m.size_original, m.parts.len());
//! }
//! # Ok(()) }
//! ```

mod client;
mod config;
mod download;
mod error;
mod model;
mod session;
mod signing;

pub use client::{jwt_exp, Client};
pub use config::{ClientConfig, Platform, Region};
pub use download::ProgressSink;
pub use error::{Error, Result};
pub use model::{Envelope, FilePart, Media, MediaId, MediaKind, MediaPage, PageCursor, Profile};
pub use session::{Session, SessionStore};
pub use signing::{Signature, Signer};
