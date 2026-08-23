//! Browser-assisted interactive login (`feature = "login"`).
//!
//! The Insta360 sign-in page is a reCAPTCHA-gated SPA, so credential login can't
//! be replayed headlessly. Instead we drive a *real* browser via the Chrome
//! DevTools Protocol: the user signs in (and solves any captcha) in the window,
//! and we read the resulting session token from the page's storage/cookies, then
//! persist it. From there the session renews itself (see `refresh`).

use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::Page;
use futures::StreamExt;
use instapi360_cloud::{session_token_exp, Session, SessionStore};

use crate::store::FileSessionStore;

const LOGIN_URL: &str =
    "https://cloud.insta360.com/account/studio_login/?v=2&studio_language=English";

/// Open a browser, wait for the user to sign in, capture and store the token.
pub async fn run(store: &FileSessionStore) -> Result<()> {
    // Use an isolated, throwaway profile so we don't attach to (and get killed
    // by) an already-running Chrome instance. chromiumoxide already applies the
    // usual automation flags (no-first-run, disable-dev-shm-usage, …).
    let user_data_dir = std::env::temp_dir().join(format!("instapi360-cdp-{}", std::process::id()));
    let config = BrowserConfig::builder()
        .with_head()
        .user_data_dir(&user_data_dir)
        .build()
        .map_err(|e| anyhow!("browser config error: {e}"))?;

    let (mut browser, mut handler) = Browser::launch(config)
        .await
        .context("launching a browser (is Chrome/Chromium installed and on PATH?)")?;

    // The handler stream must be polled for the connection to make progress.
    let handler_task = tokio::spawn(async move {
        while let Some(ev) = handler.next().await {
            if let Err(e) = ev {
                eprintln!("CDP handler error: {e:?}");
                break;
            }
        }
    });

    let page = browser
        .new_page(LOGIN_URL)
        .await
        .context("opening the login page")?;

    eprintln!("A browser window opened at the Insta360 login page.");
    eprintln!("Sign in (and solve any captcha). Waiting for the session token…");

    let result = wait_for_token(&page, Duration::from_secs(300)).await;

    let _ = browser.close().await;
    handler_task.abort();
    let _ = std::fs::remove_dir_all(&user_data_dir);

    let token = result?;
    let mut session = Session::from_token(&token);
    session.expires_at = session_token_exp(&token);
    store.save(&session)?;
    println!(
        "Logged in — token stored at {}{}",
        store.path().display(),
        session
            .expires_at
            .map(|e| format!(" (expires in {} days)", (e - now_unix()) / 86_400))
            .unwrap_or_default()
    );
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Poll the page's local/session storage and cookies until a session token
/// (a JWT carrying `openId`) appears, or the timeout elapses.
async fn wait_for_token(page: &Page, timeout: Duration) -> Result<String> {
    const STORAGE_JS: &str = r#"
        (function () {
            var out = [];
            try { for (var i = 0; i < localStorage.length; i++) { out.push(localStorage.getItem(localStorage.key(i))); } } catch (e) {}
            try { for (var i = 0; i < sessionStorage.length; i++) { out.push(sessionStorage.getItem(sessionStorage.key(i))); } } catch (e) {}
            return out.join("\n");
        })()
    "#;

    let start = Instant::now();
    loop {
        if start.elapsed() > timeout {
            bail!("timed out after {}s waiting for login", timeout.as_secs());
        }

        if let Ok(res) = page.evaluate(STORAGE_JS).await {
            if let Ok(dump) = res.into_value::<String>() {
                if let Some(tok) = find_session_token(&dump) {
                    return Ok(tok);
                }
            }
        }

        if let Ok(cookies) = page.get_cookies().await {
            let joined = cookies
                .iter()
                .map(|c| c.value.clone())
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(tok) = find_session_token(&joined) {
                return Ok(tok);
            }
        }

        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
}

/// Return the first JWT in `hay` whose payload carries `openId` (the session
/// token), ignoring other tokens the SPA may keep.
fn find_session_token(hay: &str) -> Option<String> {
    extract_jwts(hay)
        .into_iter()
        .find(|t| session_token_exp(t).is_some())
}

/// Extract candidate `eyJ….eyJ….sig` JWT substrings from arbitrary text.
fn extract_jwts(s: &str) -> Vec<String> {
    fn is_tok(c: char) -> bool {
        c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
    }
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(pos) = s[i..].find("eyJ") {
        let start = i + pos;
        let mut end = start;
        while end < bytes.len() && is_tok(bytes[end] as char) {
            end += 1;
        }
        let cand = &s[start..end];
        if cand.matches('.').count() >= 2 {
            out.push(cand.to_string());
        }
        i = end.max(start + 1);
    }
    out
}
