//! `instapi360` — CLI over `instapi360-cloud`: import a session token, inspect the
//! account, list cloud media, and download originals.
//!
//! Auth strategy ships in the order from the project plan: `import-token`
//! (working today) → `login` (headless credentials, once signing is finalized).

mod store;

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use instapi360_cloud::{
    Client, ClientConfig, MediaId, PageCursor, Platform, Region, Session, SessionStore,
};
use store::{AppConfig, FileSessionStore};
use tokio::io::BufWriter;

#[derive(Parser)]
#[command(name = "instapi360", version, about = "List and download original media from the Insta360 cloud")]
struct Cli {
    /// Cloud region.
    #[arg(long, value_enum, default_value = "global", global = true)]
    region: RegionArg,

    /// Override the per-install equipment code (X-Equipment-Code).
    #[arg(long, env = "INSTA360_EQUIPMENT_CODE", global = true)]
    equipment_code: Option<String>,

    #[command(subcommand)]
    cmd: Command,
}

#[derive(Copy, Clone, clap::ValueEnum)]
enum RegionArg {
    Global,
    Cn,
    Test,
}

impl From<RegionArg> for Region {
    fn from(r: RegionArg) -> Self {
        match r {
            RegionArg::Global => Region::Global,
            RegionArg::Cn => Region::Cn,
            RegionArg::Test => Region::Test,
        }
    }
}

#[derive(Subcommand)]
enum Command {
    /// Store an `X-User-Token` captured from the app / mitmproxy (auth 5c).
    ImportToken {
        /// The raw token value.
        token: String,
    },
    /// Log in headlessly with credentials (auth 5b — needs finalized signing).
    Login {
        #[arg(long, env = "INSTA360_EMAIL")]
        email: String,
        #[arg(long, env = "INSTA360_PASSWORD")]
        password: String,
    },
    /// Show the authenticated account profile.
    Whoami,
    /// List cloud media.
    List {
        /// Page size.
        #[arg(long, default_value_t = 50)]
        count: u32,
        /// Fetch and print all pages, not just the first.
        #[arg(long)]
        all: bool,
    },
    /// Download directly from a resolved CDN resourceKey URL (bypasses the API).
    /// Useful today: paste a SOURCE_URL from the app's download.db.
    DownloadUrl {
        /// The full https URL including `?resourceKey=...`.
        url: String,
        /// Output directory.
        #[arg(long, default_value = "./footage")]
        out: PathBuf,
        /// Resume a partial file.
        #[arg(long)]
        resume: bool,
    },
    /// Download a media item (or `all`) to a directory.
    Download {
        /// A `mediaId`, or the literal `all`.
        target: String,
        /// Output directory.
        #[arg(long, default_value = "./footage")]
        out: PathBuf,
        /// Resume partial files instead of re-downloading.
        #[arg(long)]
        resume: bool,
    },
}

fn config_dir() -> Result<PathBuf> {
    let pd = ProjectDirs::from("com", "instapi360", "instapi360")
        .ok_or_else(|| anyhow!("cannot determine config directory"))?;
    Ok(pd.config_dir().to_path_buf())
}

fn build_config(cli: &Cli, equipment_code: String) -> ClientConfig {
    let mut cfg = ClientConfig::windows(cli.region.into());
    cfg.platform = Platform::Windows;
    cfg.equipment_code = equipment_code;
    cfg
}

/// Resolve the equipment code: explicit flag/env wins (and is persisted);
/// otherwise a saved value; otherwise the auto-detected host MAC.
fn resolve_equipment_code(cli: &Cli, cfg_path: &std::path::Path) -> String {
    let mut app = AppConfig::load(cfg_path);
    if let Some(ec) = &cli.equipment_code {
        if app.equipment_code.as_deref() != Some(ec) {
            app.equipment_code = Some(ec.clone());
            let _ = app.save(cfg_path);
        }
        return ec.clone();
    }
    if let Some(ec) = app.equipment_code.clone() {
        return ec;
    }
    let detected = store::detect_equipment_code().unwrap_or_default();
    if !detected.is_empty() {
        app.equipment_code = Some(detected.clone());
        let _ = app.save(cfg_path);
    }
    detected
}

fn load_session(store: &FileSessionStore) -> Result<Session> {
    store
        .load()?
        .ok_or_else(|| anyhow!("no session — run `instapi360 import-token <TOKEN>` first"))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "insta360=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let dir = config_dir()?;
    let store = FileSessionStore::new(dir.join("session.json"));
    let equipment_code = resolve_equipment_code(&cli, &dir.join("config.json"));
    let client = Client::new(build_config(&cli, equipment_code)).context("building client")?;

    match &cli.cmd {
        Command::ImportToken { token } => {
            let session = Session::from_token(token.trim());
            store.save(&session)?;
            println!("Token stored at {}", store.path().display());
        }

        Command::Login { email: _, password: _ } => {
            return Err(anyhow!(
                "headless login not yet available — request signing is still being \
                 reverse-engineered (plan Steps 2–3). Use `import-token` for now."
            ));
        }

        Command::Whoami => {
            let session = load_session(&store)?;
            let p = client.profile(&session).await.context("fetching profile")?;
            println!(
                "user_id: {}\nemail:   {}\nname:    {}",
                p.user_id.unwrap_or_default(),
                p.email.unwrap_or_default(),
                p.name.unwrap_or_default(),
            );
        }

        Command::List { count, all } => {
            let session = load_session(&store)?;
            let mut cursor = PageCursor::first(*count);
            let mut shown = 0u64;
            loop {
                let page = client
                    .list_media(&session, cursor.clone())
                    .await
                    .context("listing media")?;
                for m in &page.items {
                    shown += 1;
                    println!(
                        "{:<28}  {:>12}  {:>2} part(s)  {}",
                        m.id,
                        human_size(m.size_original),
                        m.parts.len().max(1),
                        m.name
                    );
                }
                match (all, page.next) {
                    (true, Some(next)) => cursor = next,
                    _ => break,
                }
            }
            eprintln!("{shown} item(s) listed");
        }

        Command::DownloadUrl { url, out, resume } => {
            use instapi360_cloud::FilePart;
            tokio::fs::create_dir_all(out).await?;
            let file_name = url
                .split('?')
                .next()
                .and_then(|u| u.rsplit('/').next())
                .unwrap_or("download.bin")
                .to_string();
            // HEAD for size so the progress bar is accurate.
            let size = client
                .http()
                .head(url)
                .send()
                .await
                .ok()
                .and_then(|r| r.content_length())
                .unwrap_or(0);
            let part = FilePart { file_name: sanitize(&file_name), size, url: url.clone(), md5: None };
            let dest = out.join(&part.file_name);
            let from = if *resume {
                tokio::fs::metadata(&dest).await.map(|m| m.len()).unwrap_or(0)
            } else {
                0
            };
            let pb = ProgressBar::new(part.size.max(1));
            pb.set_style(
                ProgressStyle::with_template(
                    "{msg:<32} [{bar:32}] {bytes:>10}/{total_bytes:<10} {bytes_per_sec}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            pb.set_message(part.file_name.clone());
            pb.set_position(from);
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(from > 0)
                .truncate(from == 0)
                .open(&dest)
                .await?;
            let writer = BufWriter::new(file);
            client
                .download_part(&part, writer, from, |done, total| {
                    if let Some(t) = total {
                        pb.set_length(t);
                    }
                    pb.set_position(done);
                })
                .await?;
            pb.finish();
            println!("Saved {}", dest.display());
        }

        Command::Download { target, out, resume } => {
            let session = load_session(&store)?;
            let ids: Vec<MediaId> = if target == "all" {
                let mut cursor = PageCursor::first(100);
                let mut acc = Vec::new();
                loop {
                    let page = client.list_media(&session, cursor.clone()).await?;
                    let next = page.next.clone();
                    acc.extend(page.items.into_iter().map(|m| m.id));
                    match next {
                        Some(n) => cursor = n,
                        None => break,
                    }
                }
                acc
            } else {
                vec![MediaId(target.clone())]
            };

            tokio::fs::create_dir_all(out).await?;
            let mp = MultiProgress::new();
            for id in ids {
                download_one(&client, &session, &id, out, *resume, &mp)
                    .await
                    .with_context(|| format!("downloading {id}"))?;
            }
        }
    }

    Ok(())
}

async fn download_one(
    client: &Client,
    session: &Session,
    id: &MediaId,
    out: &std::path::Path,
    resume: bool,
    mp: &MultiProgress,
) -> Result<()> {
    let parts = client.resolve_download(session, id).await?;
    // One logical asset -> its own subdirectory, so multi-part clips stay grouped.
    let asset_dir = out.join(sanitize(&id.0));
    tokio::fs::create_dir_all(&asset_dir).await?;

    for part in &parts {
        let dest = asset_dir.join(sanitize(&part.file_name));
        let from = if resume {
            tokio::fs::metadata(&dest).await.map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        if from > 0 && part.size > 0 && from >= part.size {
            continue; // already complete
        }

        let pb = mp.add(ProgressBar::new(part.size.max(1)));
        pb.set_style(
            ProgressStyle::with_template(
                "{msg:<32} [{bar:32}] {bytes:>10}/{total_bytes:<10} {bytes_per_sec}",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        pb.set_message(part.file_name.clone());
        pb.set_position(from);

        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(from > 0)
            .truncate(from == 0)
            .open(&dest)
            .await?;
        let writer = BufWriter::new(file);

        client
            .download_part(part, writer, from, |done, total| {
                if let Some(t) = total {
                    pb.set_length(t);
                }
                pb.set_position(done);
            })
            .await?;
        pb.finish();
    }
    println!("Saved {} part(s) to {}", parts.len(), asset_dir.display());
    Ok(())
}

fn human_size(bytes: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut b = bytes as f64;
    let mut i = 0;
    while b >= 1024.0 && i < U.len() - 1 {
        b /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", U[0])
    } else {
        format!("{b:.1} {}", U[i])
    }
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect()
}
