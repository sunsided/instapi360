//! The HTTP client: header assembly, signed request dispatch, envelope
//! decoding, and the account/media/download endpoint methods.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::config::ClientConfig;
use crate::error::{Error, Result};
use crate::model::*;
use crate::session::Session;
use crate::signing::Signer;

/// A "pc"+32-hex trace id, derived from a per-request seed (splitmix64 twice).
fn trace_hex(seed: u64) -> String {
    let mut x = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let a = x ^ (x >> 31);
    let mut y = a.wrapping_add(0x9E37_79B9_7F4A_7C15);
    y = (y ^ (y >> 30)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let b = y ^ (y >> 29);
    format!("{a:016x}{b:016x}")
}

/// Async client for the Insta360 cloud API.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    cfg: ClientConfig,
    /// Retained for the share-link endpoints (the only ones that HMAC-sign).
    #[allow(dead_code)]
    signer: Signer,
    /// Account/app API host (`/account/*`, `/app/*`).
    base: String,
    /// Cloud media API host (`/cloud/service/*`) — a different host.
    cloud_base: String,
    nonce_seed: std::sync::Arc<AtomicU64>,
}

impl Client {
    /// Construct a client from config. Fails only if the TLS/HTTP stack can't
    /// be built.
    pub fn new(cfg: ClientConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(cfg.user_agent.clone())
            .build()?;
        let signer = Signer::new(&cfg);
        let base = cfg.region.openapi_base().to_string();
        let cloud_base = cfg.region.cloud_base().to_string();
        Ok(Client {
            http,
            signer,
            base,
            cloud_base,
            cfg,
            nonce_seed: std::sync::Arc::new(AtomicU64::new(0)),
        })
    }

    /// Access the underlying reqwest client (used by the download path for the
    /// OSS GET, which is *not* an API-signed request).
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    #[allow(dead_code)]
    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    /// Assemble the standard identity + auth + signing headers for an API call.
    fn build_headers(&self, session: Option<&Session>) -> Result<HeaderMap> {
        let mut h = HeaderMap::new();
        let mut put = |name: &str, val: &str| -> Result<()> {
            if val.is_empty() {
                return Ok(());
            }
            let n = HeaderName::from_bytes(name.as_bytes())
                .map_err(|e| Error::Signing(format!("bad header name {name}: {e}")))?;
            let v = HeaderValue::from_str(val)
                .map_err(|e| Error::Signing(format!("bad header value for {name}: {e}")))?;
            h.insert(n, v);
            Ok(())
        };

        // Client identity — header names/values taken verbatim from the live
        // desktop app's outgoing requests (recovered from process memory).
        put("app_version", &self.cfg.app_version)?;
        put("app_traffic", &self.cfg.app_traffic)?;
        put("X-Equipment-Code", &self.cfg.equipment_code)?;
        put("x-insta360-platform", self.cfg.platform.header_value())?; // "pc/windows"
        put("X-Language", &self.cfg.language)?;
        put("X-MainLand", self.cfg.region.mainland_flag())?;
        put("Referer", "https://cloud.insta360.com/")?;
        // Trace id format observed: "pc" + 32 lowercase hex.
        let seed = self.nonce_seed.fetch_add(1, Ordering::Relaxed);
        put("X-Insta360-Trace-Id", &format!("pc{}", trace_hex(seed)))?;

        // Auth. The desktop app sends the session JWT in the `Authentication`
        // header; some endpoints also accept `X-User-Token`. Send both so we
        // work regardless of which the endpoint checks.
        if let Some(s) = session {
            put("Authentication", &s.user_token)?;
            put("X-User-Token", &s.user_token)?;
        }

        Ok(h)
    }

    /// Send a JSON API request and decode the `data` payload from the envelope.
    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: &str,
        session: Option<&Session>,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<T> {
        let headers = self.build_headers(session)?;
        let mut req = self.http.request(method, url).headers(headers);
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::Auth(format!("401 from {url}")));
        }

        let env: Envelope<T> = serde_json::from_str(&text).map_err(|e| Error::Api {
            code: status.as_u16() as i64,
            message: format!(
                "bad response from {url} (http {status}): {e}; body: {}",
                text.chars().take(200).collect::<String>()
            ),
        })?;

        if !env.is_ok() {
            let code = env.code.unwrap_or(-1);
            // The API uses a dedicated code for token expiry; treat as auth.
            let msg = env.message.unwrap_or_default();
            if code == 401 || msg.to_lowercase().contains("token") {
                return Err(Error::Auth(format!("api code {code}: {msg}")));
            }
            return Err(Error::Api { code, message: msg });
        }

        env.data.ok_or_else(|| Error::Api {
            code: 0,
            message: "empty data".into(),
        })
    }

    // ----- account -----

    /// Fetch the authenticated profile
    /// (`GET openapi-*/account/v2/getProfile`). The account API lives on the
    /// openapi host; `data` nests the profile under `account`.
    pub async fn profile(&self, session: &Session) -> Result<Profile> {
        let url = format!("{}/account/v2/getProfile", self.base);
        let raw: Value = self
            .request(Method::GET, &url, Some(session), &[], None)
            .await?;
        let obj = raw.get("account").unwrap_or(&raw);
        // Parse leniently — the account object carries overlapping name-ish keys
        // (name/nickname/…) that trip serde's alias de-dup, so pick by hand.
        Ok(Profile {
            user_id: as_str(obj, &["userId", "ins_user_id", "id", "accountId", "openId"]),
            email: as_str(obj, &["userEmail", "email", "bindEmail"]),
            name: as_str(
                obj,
                &["nickName", "nickname", "userName", "username", "name"],
            ),
        })
    }

    /// Renew the session (`GET openapi-*/account/v2/refreshToken`).
    ///
    /// The current token authenticates the call and the API mints a fresh one
    /// in `data.token` — no separate refresh token is required, so a session can
    /// be rolled forward indefinitely as long as it is renewed before it lapses.
    pub async fn refresh(&self, session: &Session) -> Result<Session> {
        #[derive(serde::Deserialize)]
        struct RefreshData {
            token: String,
        }
        let url = format!("{}/account/v2/refreshToken", self.base);
        let d: RefreshData = self
            .request(Method::GET, &url, Some(session), &[], None)
            .await?;
        let expires_at = jwt_exp(&d.token);
        Ok(Session {
            user_token: d.token,
            refresh_token: session.refresh_token.clone(),
            expires_at,
            user_id: session.user_id.clone(),
        })
    }

    // ----- media listing -----

    /// List cloud media, one page
    /// (`GET service-*/cloud/service/media/view/list?pageNumber&pageSize`).
    pub async fn list_media(&self, session: &Session, cursor: PageCursor) -> Result<MediaPage> {
        let url = format!("{}/cloud/service/media/view/list", self.cloud_base);
        let q = [
            ("pageNumber", cursor.page.to_string()),
            ("pageSize", cursor.count.to_string()),
        ];
        let raw: Value = self
            .request(Method::GET, &url, Some(session), &q, None)
            .await?;
        parse_media_page(&raw, &cursor)
    }

    /// Fetch full detail for one media item
    /// (`GET service-*/cloud/service/media/view/detail?mediaId`).
    pub async fn media_detail(&self, session: &Session, id: &MediaId) -> Result<Media> {
        let url = format!("{}/cloud/service/media/view/detail", self.cloud_base);
        let q = [("mediaId", id.0.clone())];
        let raw: Value = self
            .request(Method::GET, &url, Some(session), &q, None)
            .await?;
        parse_media(&raw).ok_or_else(|| Error::Api {
            code: 0,
            message: "detail had no media".into(),
        })
    }

    /// Resolve the signed CDN download URL(s) for a media item
    /// (`GET service-*/cloud/service/media/download?mediaId`). Returns one
    /// [`FilePart`] per file (the response's `data.paths[]`).
    ///
    /// The `resourceKey` URLs are short-lived (~24h) — download promptly.
    pub async fn resolve_download(&self, session: &Session, id: &MediaId) -> Result<Vec<FilePart>> {
        let url = format!("{}/cloud/service/media/download", self.cloud_base);
        let q = [("mediaId", id.0.clone())];
        let raw: Value = self
            .request(Method::GET, &url, Some(session), &q, None)
            .await?;
        let parts = parse_file_parts(&raw);
        if parts.is_empty() {
            return Err(Error::Api {
                code: 0,
                message: "download response had no file parts".into(),
            });
        }
        Ok(parts)
    }
}

// ----- lenient JSON parsers (shapes vary across API versions) -----

fn as_u64(v: &Value, keys: &[&str]) -> Option<u64> {
    for k in keys {
        if let Some(n) = v.get(k) {
            if let Some(u) = n.as_u64() {
                return Some(u);
            }
            if let Some(s) = n.as_str() {
                if let Ok(u) = s.parse() {
                    return Some(u);
                }
            }
        }
    }
    None
}

fn as_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

/// Decode a JWT's `exp` (unix seconds) without verifying the signature.
pub fn jwt_exp(token: &str) -> Option<i64> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp").and_then(|x| x.as_i64())
}

fn classify(media_type: Option<&str>, name: &str) -> MediaKind {
    let lower = name.to_lowercase();
    if lower.ends_with(".insv") {
        return MediaKind::Insv360;
    }
    if lower.ends_with(".insp") || lower.ends_with(".jpg") || lower.ends_with(".dng") {
        return MediaKind::Photo;
    }
    if lower.ends_with(".mp4") {
        return MediaKind::FlatVideo;
    }
    match media_type {
        Some(t) if t.contains("photo") || t.contains("image") => MediaKind::Photo,
        Some(t) if t.contains("video") => MediaKind::FlatVideo,
        _ => MediaKind::Other,
    }
}

/// Extract the file parts from a media/detail/download JSON object.
fn parse_file_parts(v: &Value) -> Vec<FilePart> {
    // `paths` is the download-resolve shape ([{index,name,size,url}]); the
    // others cover detail/upload variants.
    let arr = [
        "paths",
        "files",
        "fileList",
        "mediaUploadRespList",
        "downloadList",
    ]
    .iter()
    .find_map(|k| v.get(k).and_then(|x| x.as_array()))
    // Some responses nest under `data`.
    .or_else(|| {
        v.get("data").and_then(|d| {
            ["paths", "files", "fileList"]
                .iter()
                .find_map(|k| d.get(k).and_then(|x| x.as_array()))
        })
    });

    if let Some(items) = arr {
        return items
            .iter()
            .filter_map(|it| {
                let url = as_str(it, &["download_url", "downloadUrl", "url", "downloadPath"])?;
                let name = as_str(it, &["fileName", "file_name", "name"])
                    .unwrap_or_else(|| url.rsplit('/').next().unwrap_or("part").to_string());
                let size = as_u64(it, &["fileSize", "size", "mediaSize"]).unwrap_or(0);
                let md5 = as_str(it, &["md5", "fileMd5", "coverMd5"]);
                Some(FilePart {
                    file_name: name,
                    size,
                    url,
                    md5,
                })
            })
            .collect();
    }

    // Single-file shape: a bare download_url on the object itself.
    if let Some(url) = as_str(v, &["download_url", "downloadUrl", "url", "downloadPath"]) {
        let name = as_str(v, &["mediaName", "fileName", "name"])
            .unwrap_or_else(|| url.rsplit('/').next().unwrap_or("media").to_string());
        let size = as_u64(v, &["downOriginalSize", "fileSize", "mediaSize"]).unwrap_or(0);
        let md5 = as_str(v, &["md5", "coverMd5"]);
        return vec![FilePart {
            file_name: name,
            size,
            url,
            md5,
        }];
    }

    Vec::new()
}

/// Parse a single media object (list/detail item, field names from the live API).
fn parse_media(v: &Value) -> Option<Media> {
    let id = as_str(v, &["mediaId", "id", "uuid"])?;
    let name = as_str(v, &["mediaName", "name", "fileName"]).unwrap_or_else(|| id.clone());
    let media_type = as_str(v, &["mediaType", "type"]);
    let kind = classify(media_type.as_deref(), &name);
    let size = as_u64(
        v,
        &["mediaSize", "downOriginalSize", "fileSize", "file_size"],
    )
    .unwrap_or(0);
    // Times are unix milliseconds in this API (e.g. createTime 1787429512249).
    let time = as_u64(
        v,
        &["createTime", "mediaTime", "create_time", "upload_time_s"],
    )
    .map(|u| {
        if u > 1_000_000_000_000 {
            (u / 1000) as i64
        } else {
            u as i64
        }
    });
    let camera = as_str(v, &["cameraType", "camera_name", "cameraName", "camera"]);

    // Prefer the download-resolve `paths` (with URLs); otherwise synthesize
    // name-only parts from `fileItems` (array of filenames) so the part count
    // and multi-file grouping are correct even in a plain listing.
    let mut parts = parse_file_parts(v);
    if parts.is_empty() {
        if let Some(items) = v.get("fileItems").and_then(|x| x.as_array()) {
            parts = items
                .iter()
                .filter_map(|x| x.as_str())
                .map(|n| FilePart {
                    file_name: n.to_string(),
                    size: 0,
                    url: String::new(),
                    md5: None,
                })
                .collect();
        }
    }
    Some(Media {
        id: MediaId(id),
        name,
        kind,
        size_original: size,
        time_unix: time,
        camera,
        parts,
    })
}

/// Parse a list response into a page.
fn parse_media_page(v: &Value, cursor: &PageCursor) -> Result<MediaPage> {
    let arr = ["list", "mediaVos", "mediaInfos", "medias", "items"]
        .iter()
        .find_map(|k| v.get(k).and_then(|x| x.as_array()))
        .cloned()
        .or_else(|| v.as_array().cloned())
        .unwrap_or_default();

    let items: Vec<Media> = arr.iter().filter_map(parse_media).collect();
    let total = as_u64(v, &["total_count", "totalCount", "total", "count"]);
    let got_so_far = (cursor.page as u64) * (cursor.count as u64);
    let next = match total {
        Some(t) if got_so_far < t && !items.is_empty() => Some(cursor.next()),
        None if items.len() as u32 == cursor.count => Some(cursor.next()),
        _ => None,
    };
    Ok(MediaPage { items, total, next })
}
