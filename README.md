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
# 1. store your own session token (from the desktop/mobile app you're signed into)
instapi360 import-token "<session-token>"

# 2. use it — the equipment code auto-detects your host and is remembered
instapi360 whoami
instapi360 list --all
instapi360 download <mediaId> --out ./footage      # grabs all parts (.insv + .lrv)
instapi360 download all --out ./footage --resume
```

## How it works
- Authentication is your session token alone (sent as a header); there is no
  extra request signing for listing or downloading.
- Listing and download resolution are simple authenticated GET requests.
- Downloads resolve to short-lived CDN URLs and stream with **resumable, ranged**
  transfers; multi-file media (video + proxy, or dual-lens parts) are grouped and
  saved together as one asset.

## Design
`instapi360-cloud` is deliberately dependency-light and platform-agnostic
(`rustls`, caller-provided `AsyncWrite` sinks and a `SessionStore` trait) so it
can be embedded in other apps — desktop or mobile — not just this CLI.

## Roadmap
- Headless email/password login.
- Media metadata (camera, timestamps, gyro/immersion data) surfaced in `list`.

## License
Licensed under either of MIT or Apache-2.0 at your option.

## Naming
`instapi360` is a working name and may change; it is not an official product name.
