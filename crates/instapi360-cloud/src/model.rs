//! Wire and domain types for cloud media.
//!
//! The API wraps every response in an envelope `{ code, ... , data }`. Media
//! items can be **multi-file** (an X5 dual-lens `.insv` is two files,
//! `VID_..._00_*.insv` + `VID_..._10_*.insv`; an Ace Pro 2 clip is one). We
//! model one logical [`Media`] as an asset owning a list of [`FilePart`]s.

use serde::{Deserialize, Serialize};

/// Standard response envelope. Insta360 has used both `code` and `errCode`
/// across versions, and both `errInfo`/`msg` for the message; accept all.
#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    #[serde(alias = "errCode")]
    pub code: Option<i64>,
    #[serde(alias = "errInfo", alias = "msg", alias = "message")]
    pub message: Option<String>,
    pub data: Option<T>,
}

impl<T> Envelope<T> {
    /// `true` when the envelope reports success (`code` is 0 or absent).
    pub fn is_ok(&self) -> bool {
        self.code.unwrap_or(0) == 0
    }
}

/// Opaque identifier for a cloud media item.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaId(pub String);

impl std::fmt::Display for MediaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Kind of media, derived from the API's `mediaType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    /// 360 footage (`.insv`), single- or dual-lens.
    Insv360,
    /// Flat / standard video (`.mp4`).
    FlatVideo,
    /// Photo (`.insp`/`.jpg`/`.dng`).
    Photo,
    /// Unknown / not yet mapped.
    Other,
}

/// One downloadable file belonging to a [`Media`]. For dual-lens 360 clips
/// there are two of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePart {
    /// Original filename, e.g. `VID_20250514_192854_00_005.insv`.
    pub file_name: String,
    /// Size in bytes as reported by the API (`fileSize`).
    pub size: u64,
    /// Signed download URL (Alibaba OSS). Short-lived — resolve just-in-time.
    pub url: String,
    /// Expected MD5 if the API provides one, for integrity checking.
    pub md5: Option<String>,
}

/// A logical cloud media asset (may span multiple [`FilePart`]s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub id: MediaId,
    /// Display / base name (`mediaName`).
    pub name: String,
    pub kind: MediaKind,
    /// Original size in bytes summed across parts (`downOriginalSize`).
    pub size_original: u64,
    /// Capture time (unix seconds) if known (`mediaTime`/`create_time`).
    pub time_unix: Option<i64>,
    /// Camera model if the API reports it (`camera_name`).
    pub camera: Option<String>,
    /// The files that make up this asset.
    pub parts: Vec<FilePart>,
}

/// One page of a media listing plus the cursor to fetch the next page.
#[derive(Debug, Clone)]
pub struct MediaPage {
    pub items: Vec<Media>,
    /// Total item count reported by the API (`totalCount`), if present.
    pub total: Option<u64>,
    /// Opaque cursor / next page token; `None` when there are no more pages.
    pub next: Option<PageCursor>,
}

/// Pagination request cursor.
#[derive(Debug, Clone, Default)]
pub struct PageCursor {
    /// Page index (1-based) or offset, depending on the endpoint.
    pub page: u32,
    /// Page size.
    pub count: u32,
}

impl PageCursor {
    pub fn first(count: u32) -> Self {
        PageCursor { page: 1, count }
    }
    pub fn next(&self) -> Self {
        PageCursor { page: self.page + 1, count: self.count }
    }
}

/// Authenticated account profile (subset we need).
#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    #[serde(alias = "userId", alias = "ins_user_id")]
    pub user_id: Option<String>,
    #[serde(alias = "userEmail", alias = "email")]
    pub email: Option<String>,
    #[serde(alias = "userName", alias = "username", alias = "nickname")]
    pub name: Option<String>,
}
