//! Streaming, resumable downloads of file parts from OSS signed URLs.
//!
//! The core [`download_part`] takes any `AsyncWrite` sink and an optional
//! starting offset (HTTP `Range`) so it serves both the CLI (writing to a file)
//! and a future Android caller without the core crate touching the filesystem.

use futures::StreamExt;
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::model::FilePart;

/// Progress callback: `(bytes_done, total_bytes_if_known)`.
pub trait ProgressSink: FnMut(u64, Option<u64>) {}
impl<T: FnMut(u64, Option<u64>)> ProgressSink for T {}

impl Client {
    /// Stream one [`FilePart`] into `sink`, resuming from byte `from`.
    ///
    /// * `from` — resume offset; pass 0 for a fresh download. Sent as an HTTP
    ///   `Range: bytes=from-` header.
    /// * `progress` — invoked as bytes arrive with the running total.
    ///
    /// On a `403` (OSS URL expired) the caller should re-`resolve_download` and
    /// retry with the new URL and the same `from`.
    pub async fn download_part<W, P>(
        &self,
        part: &FilePart,
        mut sink: W,
        from: u64,
        mut progress: P,
    ) -> Result<()>
    where
        W: AsyncWrite + Unpin,
        P: ProgressSink,
    {
        let mut req = self.http().get(&part.url);
        if from > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={from}-"));
        }
        let resp = req.send().await?;
        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::Auth(
                "OSS url expired (403) — re-resolve download".into(),
            ));
        }
        let resp = resp.error_for_status()?;

        // Total = declared part size when known, else content-length + offset.
        let content_len = resp.content_length();
        let total = if part.size > 0 {
            Some(part.size)
        } else {
            content_len.map(|c| c + from)
        };

        let mut done = from;
        progress(done, total);

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            sink.write_all(&chunk).await?;
            done += chunk.len() as u64;
            progress(done, total);
        }
        sink.flush().await?;

        if let Some(t) = total {
            if done < t {
                return Err(Error::Partial {
                    got: done,
                    expected: t,
                });
            }
        }
        Ok(())
    }
}
