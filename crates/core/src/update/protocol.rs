use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::UpdateChannel;

/// GitHub Releases API for PlotX. `PLOTX_UPDATE_URL` can redirect requests for
/// development and testing.
pub const DEFAULT_SERVER_URL: &str = "https://api.github.com/repos/nmrtist/plotx";

/// Effective update server base URL (no trailing slash).
pub fn server_url() -> String {
    let url = std::env::var("PLOTX_UPDATE_URL").unwrap_or_else(|_| DEFAULT_SERVER_URL.to_owned());
    url.trim_end_matches('/').to_owned()
}

/// The server's answer to "what is the latest release on this channel for
/// this platform": a version, a direct download URL, and the artifact's
/// SHA-256 so the download can be verified before it is ever executed.
#[derive(Clone, Debug, Deserialize)]
pub struct ReleaseAsset {
    pub version: String,
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("update server request failed: {0}")]
    Http(String),
    #[error("update server sent an invalid response: {0}")]
    Protocol(String),
    #[error("GitHub rate limit reached; retry in {retry_after:?}")]
    RateLimited { retry_after: Duration },
    #[error("invalid version string {version:?}: {source}")]
    Version {
        version: String,
        source: semver::Error,
    },
    #[error("downloaded file failed its integrity check")]
    Integrity,
    #[error("couldn't write the downloaded update: {0}")]
    Io(#[from] std::io::Error),
}

impl UpdateError {
    pub(super) fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after } => Some(*retry_after),
            _ => None,
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .user_agent(concat!("plotx/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Ask GitHub for the latest release on `channel` for this platform.
pub fn check_latest(
    base_url: &str,
    channel: UpdateChannel,
) -> Result<Option<ReleaseAsset>, UpdateError> {
    let base_url = base_url.trim_end_matches('/');
    let mut latest: Option<(semver::Version, GithubRelease)> = None;
    let mut page_number = 1_u64;
    loop {
        let url = format!("{base_url}/releases?per_page=100&page={page_number}");
        let body = github_get(&url)?
            .into_string()
            .map_err(|e| UpdateError::Protocol(e.to_string()))?;
        let page: Vec<GithubRelease> =
            serde_json::from_str(&body).map_err(|e| UpdateError::Protocol(e.to_string()))?;
        let exhausted = page.len() < 100;
        for release in page.into_iter().filter(|release| !release.draft) {
            let Ok(version) = parse_version(&release.tag_name) else {
                continue;
            };
            if channel_matches(&version, channel)
                && latest
                    .as_ref()
                    .is_none_or(|(candidate, _)| version > *candidate)
            {
                latest = Some((version, release));
            }
        }
        if exhausted {
            break;
        }
        page_number += 1;
    }
    let Some((version, release)) = latest else {
        return Ok(None);
    };

    let filename = format!(
        "plotx-{}-{}-{}.zip",
        version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let Some(asset) = release.assets.iter().find(|asset| asset.name == filename) else {
        return Ok(None);
    };
    let sha256 = match asset.digest.as_deref().and_then(sha256_digest) {
        Some(hash) => hash.to_owned(),
        None => checksum_from_release(&release, &filename)?,
    };
    Ok(Some(ReleaseAsset {
        version: version.to_string(),
        url: asset.browser_download_url.clone(),
        sha256,
    }))
}

fn github_get(url: &str) -> Result<ureq::Response, UpdateError> {
    match agent()
        .get(url)
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
    {
        Ok(response) => Ok(response),
        Err(ureq::Error::Status(status, response))
            if status == 429
                || (status == 403
                    && (response.header("Retry-After").is_some()
                        || response.header("X-RateLimit-Remaining") == Some("0"))) =>
        {
            Err(UpdateError::RateLimited {
                retry_after: rate_limit_delay(&response),
            })
        }
        Err(ureq::Error::Status(status, response)) => Err(UpdateError::Http(format!(
            "HTTP {status} {}",
            response.status_text()
        ))),
        Err(error) => Err(UpdateError::Http(error.to_string())),
    }
}

fn rate_limit_delay(response: &ureq::Response) -> Duration {
    if let Some(seconds) = response
        .header("Retry-After")
        .and_then(|value| value.parse::<u64>().ok())
    {
        return Duration::from_secs(seconds.max(1));
    }
    if let Some(reset) = response
        .header("X-RateLimit-Reset")
        .and_then(|value| value.parse::<u64>().ok())
    {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        return Duration::from_secs(reset.saturating_sub(now).max(1));
    }
    Duration::from_secs(60 * 60)
}

fn checksum_from_release(release: &GithubRelease, filename: &str) -> Result<String, UpdateError> {
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")
        .ok_or_else(|| UpdateError::Protocol("release has no SHA256SUMS asset".into()))?;
    let body = github_get(&checksums.browser_download_url)?
        .into_string()
        .map_err(|e| UpdateError::Protocol(e.to_string()))?;
    body.lines()
        .find_map(|line| {
            let (hash, name) = line.split_once(char::is_whitespace)?;
            let name = name
                .trim_start()
                .strip_prefix('*')
                .unwrap_or(name.trim_start());
            (name == filename).then(|| sha256_digest(hash).map(str::to_owned))?
        })
        .ok_or_else(|| UpdateError::Protocol(format!("SHA256SUMS has no entry for {filename}")))
}

fn sha256_digest(value: &str) -> Option<&str> {
    let hash = value.strip_prefix("sha256:").unwrap_or(value);
    (hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(hash)
}

fn channel_matches(version: &semver::Version, channel: UpdateChannel) -> bool {
    let prerelease_channel = version.pre.as_str().split('.').next().unwrap_or("");
    match channel {
        UpdateChannel::Stable => version.pre.is_empty(),
        UpdateChannel::Beta => prerelease_channel == "beta",
        UpdateChannel::Alpha => prerelease_channel == "alpha",
    }
}

/// Whether `fetched` is a strict semver upgrade over `installed`. Prerelease
/// ordering does the channel-appropriate thing: `0.2.0-alpha.2 > 0.2.0-alpha.1`
/// and `0.2.0 > 0.2.0-beta.3`, while downgrades never qualify.
pub fn is_newer(fetched: &str, installed: &str) -> Result<bool, UpdateError> {
    Ok(parse_version(fetched)? > parse_version(installed)?)
}

fn parse_version(version: &str) -> Result<semver::Version, UpdateError> {
    semver::Version::parse(version.trim().trim_start_matches('v')).map_err(|source| {
        UpdateError::Version {
            version: version.to_owned(),
            source,
        }
    })
}

/// Directory downloaded updates land in. Recreated empty before each download
/// so a failed or superseded attempt never leaves a stale artifact behind.
pub(super) fn download_dir() -> PathBuf {
    static NEXT_OPERATION: AtomicU64 = AtomicU64::new(0);
    let operation = NEXT_OPERATION.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("plotx-update-{}-{operation}", std::process::id()))
}

/// Download `asset` and verify its SHA-256 before reporting success. Progress
/// is reported as whole percentages via `progress` (`None` when the server
/// sends no Content-Length).
pub fn download(
    asset: &ReleaseAsset,
    mut progress: impl FnMut(Option<u8>),
) -> Result<PathBuf, UpdateError> {
    let dir = download_dir();
    std::fs::create_dir_all(&dir)?;

    let response = agent()
        .get(&asset.url)
        .call()
        .map_err(|e| UpdateError::Http(e.to_string()))?;
    let total: Option<u64> = response
        .header("Content-Length")
        .and_then(|v| v.parse().ok());

    let filename = asset
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty() && !name.contains(['\\', ':']))
        .unwrap_or("plotx-update.bin");
    let path = dir.join(filename);

    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(&path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut read_total: u64 = 0;
    let mut last_pct: Option<u8> = None;
    loop {
        let n = reader.read(&mut buf).map_err(UpdateError::Io)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        read_total += n as u64;
        let pct = total.map(|t| ((read_total.min(t) * 100) / t.max(1)) as u8);
        if pct != last_pct {
            last_pct = pct;
            progress(pct);
        }
    }
    file.flush()?;
    drop(file);

    let actual = hex_string(&hasher.finalize());
    if !actual.eq_ignore_ascii_case(asset.sha256.trim()) {
        // Never leave a file that failed verification where a later step
        // could mistake it for a good download.
        let _ = std::fs::remove_file(&path);
        return Err(UpdateError::Integrity);
    }
    Ok(path)
}

pub(super) fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
