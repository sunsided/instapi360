# instapi360

A small Rust library (`instapi360-cloud`) and CLI (`instapi360`) to access **your
own** Insta360 cloud media from the command line: sign in with your existing
session, list your uploads, and download the **original** camera files
(360 `.insv` + `.lrv` proxy, flat `.mp4`) for local editing and archival.

> **Not affiliated with Insta360.** This project is an independent, unofficial
> tool. It is **not** affiliated with, endorsed by, sponsored by, or connected
> to Insta360 or Arashivision. "Insta360", "Ace Pro", "X5" and related names are
> trademarks of their respective owners and are used here only nominatively to
> describe interoperability.
>
> **Personal use only.** Intended solely for accessing **your own account** and
> **your own footage** that you have the right to access. You are responsible
> for complying with Insta360's Terms of Service and applicable law. Do not use
> it to access anyone else's data.

## Workspace
| crate | role |
|-------|------|
| `instapi360-cloud` | Async client library: auth, list, resolve, download. rustls-only, I/O injected by the caller (portable, no filesystem assumptions in the core). |
| `instapi360-cli`   | The `instapi360` binary over the library. |

## Build & test
```sh
cargo build
cargo test -p instapi360-cloud
cargo run -p instapi360-cli -- --help
```

## Usage
```sh
# 1a. interactive login — opens a browser, you sign in (and solve any captcha),
#     the token is captured automatically  (needs Chrome/Chromium)
instapi360 login
# 1b. …or paste a token you already have
instapi360 import-token "<session-token>"

# 2. use it — equipment code auto-detects your host and is remembered
instapi360 whoami
instapi360 list --all
instapi360 download <mediaId> --out ./footage      # grabs all parts (.insv + .lrv)
instapi360 download all --out ./footage --resume

# 3. keep the session alive headlessly (no re-login)
instapi360 refresh        # or let commands auto-renew when near expiry
```

The `login` command is behind the **`login`** cargo feature, **on by default**.
It pulls a Chrome-automation dependency; for a lean, token-import-only build:
`cargo build --no-default-features`.

## Signing in

**Requirement:** Chrome or Chromium installed (used only for the login window;
the rest of the CLI needs no browser).

### Browser login (recommended)
```sh
instapi360 login
```
1. A **fresh** Chrome window opens on the Insta360 sign-in page. It uses a
   throwaway profile (removed afterwards), so it won't collide with a Chrome you
   already have running — and you sign in from scratch in this window.
2. Sign in with your account, solving any reCAPTCHA challenge that appears.
3. Once you're signed in, the CLI reads the session token from the page and
   stores it at `~/.config/instapi360/session.json`, then closes the browser.
   You'll see: `Logged in — token stored … (expires in 30 days)`.

The window waits up to 5 minutes for you to finish. Nothing is typed for you —
you enter your own credentials directly into Insta360's page.

### Manual token import (no browser)
If you already have a session token (e.g. from a build without the `login`
feature), store it directly:
```sh
instapi360 import-token "<session-token>"
```

### Staying signed in
The token lasts ~30 days and **renews itself**: `whoami`, `list` and `download`
auto-renew it when it's within a few days of expiring, and `instapi360 refresh`
renews on demand. So `login` is a once-in-a-while action, not per-use.

### Signing out
Delete the stored session to sign out:
```sh
rm ~/.config/instapi360/session.json
```

### Troubleshooting
- **`oneshot canceled` / no window** — usually an outdated Chrome-automation
  stack against a very new Chrome. Update the toolchain (`cargo update`) and
  ensure Chrome/Chromium is current.
- **Headless/server box** — `login` needs a display for the browser window. On a
  headless machine, run `login` on a desktop, copy `session.json` over, or use
  `import-token`.

## How it works
- **Sign in once, stay signed in.** Authentication is your session token alone
  (sent as a header) — no request signing. The token lives ~30 days and is
  renewed via `/account/v2/refreshToken`, which mints a fresh token from the
  current one (no separate refresh token). `whoami`/`list`/`download`
  auto-renew when the token is within a few days of expiring, so a single
  import keeps working indefinitely.
- The sign-in endpoint is reCAPTCHA-gated, so a headless credential POST won't
  work. `login` therefore drives a **real browser** (via the Chrome DevTools
  Protocol): you sign in and solve any challenge, and the session token is read
  back from the page and stored. `import-token` is the manual alternative.
- Listing and download resolution are authenticated GET requests. Downloads
  resolve to short-lived CDN URLs and stream with **resumable, ranged**
  transfers; multi-file media (video + proxy, or dual-lens parts) are grouped
  and saved together as one asset.

## Design
`instapi360-cloud` is deliberately dependency-light and platform-agnostic
(`rustls`, caller-provided `AsyncWrite` sinks and a `SessionStore` trait) so it
can be embedded in other apps — desktop or mobile — not just this CLI.

## Roadmap
- Media metadata (camera, timestamps, gyro/immersion data) surfaced in `list`.

## License
Licensed under either of MIT or Apache-2.0 at your option.

## Naming
`instapi360` is a working name and may change; it is not an official product name.
