//! Client configuration: which cloud environment and how the client identifies
//! itself. Values that are still being reverse-engineered are marked below and
//! centralized here so a single edit wires them in once capture confirms them.

use std::fmt;

/// Cloud environment / region. The base host is region-selected; this user's
/// desktop `startup.ini` reported `current_area=OverSea` →
/// `HOST_OPENAPI_INSTA360=openapi-g.insta360.com`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Region {
    /// Global / OverSea: `openapi-g.insta360.com`.
    #[default]
    Global,
    /// Mainland China: `openapi.insta360.com`.
    Cn,
    /// Test environment: `openapi-test.insta360.com`.
    Test,
}

impl Region {
    /// Base origin for the **account/app** API (`/account/*`, `/app/*`).
    pub fn openapi_base(self) -> &'static str {
        match self {
            Region::Global => "https://openapi-g.insta360.com",
            Region::Cn => "https://openapi.insta360.com",
            Region::Test => "https://openapi-test.insta360.com",
        }
    }

    /// Base origin for the **cloud media** API (`/cloud/service/*`). This is a
    /// different host from the account API — verified: list/download 404 on
    /// openapi-g but serve from the regional `service-*` host.
    pub fn cloud_base(self) -> &'static str {
        match self {
            // "fra" = Frankfurt (OverSea/EU). Other regions' hosts differ; this
            // is the confirmed Global one.
            Region::Global => "https://service-fra.insta360.com",
            Region::Cn => "https://service.insta360.com",
            Region::Test => "https://service-test.insta360.com",
        }
    }

    /// Value sent in the `X-MainLand` header (`1` for CN, `0` otherwise).
    pub fn mainland_flag(self) -> &'static str {
        match self {
            Region::Cn => "1",
            _ => "0",
        }
    }
}

/// The platform value sent in `x-insta360-platform` and used to pick the
/// client-identity constants. The desktop app sends `windows`; a future Android
/// build sends `android`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Android,
}

impl Platform {
    /// Value for `x-insta360-platform`. The desktop app sends `pc/windows`
    /// (verbatim from captured requests), not just `windows`.
    pub fn header_value(self) -> &'static str {
        match self {
            Platform::Windows => "pc/windows",
            Platform::Android => "android",
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.header_value())
    }
}

/// Static client identity + request-signing material.
///
/// NOTE (reverse-engineering pending): `app_key`, `client_id` and `sign_key`
/// are not hardcoded literals in the desktop binary — they are recovered from
/// mitmproxy capture / APK analysis (see the project plan, Steps 2–3). Until
/// then they default to empty and signing will return [`crate::Error::Signing`].
#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub region: Region,
    pub platform: Platform,
    /// `X-APP-VERSION` — e.g. the Studio build `6.0.2` or the app version.
    pub app_version: String,
    /// `X-APP` — short app identifier.
    pub app_id: String,
    /// `app_traffic` header — release channel (e.g. `release`).
    pub app_traffic: String,
    /// `X-Client-Id`.
    pub client_id: String,
    /// `X-MCS-AppKey` — the app key sent with signed requests.
    pub app_key: String,
    /// Per-install `X-Equipment-Code`. Persisted by the caller across runs.
    pub equipment_code: String,
    /// UI language sent as `X-Language` (e.g. `en`).
    pub language: String,
    /// HMAC key for request signing (`client_key`/secret). Pending capture.
    pub sign_key: String,
    /// `User-Agent` header.
    pub user_agent: String,
}

impl ClientConfig {
    /// A skeleton config for the desktop (`windows`) client. Identity/signing
    /// fields are left blank and must be filled from capture before signed
    /// endpoints will work; token-import + download of already-signed OSS URLs
    /// works without them.
    pub fn windows(region: Region) -> Self {
        ClientConfig {
            region,
            platform: Platform::Windows,
            app_version: "6.0.2".to_string(),
            app_id: "studio_win".to_string(),
            app_traffic: "release".to_string(),
            client_id: String::new(),
            app_key: String::new(),
            equipment_code: String::new(),
            language: "en".to_string(),
            sign_key: String::new(),
            user_agent: "insta360/studio".to_string(),
        }
    }

    /// True once the fields required to sign an API request are populated.
    pub fn can_sign(&self) -> bool {
        !self.app_key.is_empty() && !self.sign_key.is_empty()
    }
}
