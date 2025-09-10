use crate::app::utils::zv_agent;
use crate::{NetErr, ZigVersion, ZvError, tools};
use color_eyre::eyre::{Result, WrapErr, eyre};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Url;
use std::sync::LazyLock;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use yansi::Paint;
mod mirror;
use mirror::*;

/// Cache strategy for index loading
#[derive(Debug, Clone, Copy)]
pub enum CacheStrategy {
    /// Always fetch fresh data from network
    AlwaysRefresh,
    /// Use cached data if available, only fetch if no cache exists
    PreferCache,
    /// Respect TTL - use cache if not expired, otherwise refresh
    RespectTtl,
}

const TARGET: &str = "zv::network";
/// 24 hours default TTL for index
pub static INDEX_TTL_HOURS: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("ZV_INDEX_TTL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(24)
});
/// 48 hours default TTL for mirrors list
pub static MIRRORS_TTL_HOURS: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("ZV_MIRRORS_TTL_HOURS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(48)
});

#[derive(Debug, Clone)]
pub struct ZvNetwork {
    client: reqwest::Client,
    mirror_manager: MirrorManager,
    base_path: PathBuf,
}

impl ZvNetwork {
    pub async fn new(zv_base_path: impl AsRef<Path>) -> Result<Self, ZvError> {
        let client = reqwest::Client::builder()
            .user_agent(zv_agent())
            .timeout(std::time::Duration::from_secs(30)) // 30 second timeout
            .build()
            .expect("Failed to build HTTP client");
        let mirrors_path = zv_base_path.as_ref().join("mirrors.toml");
        let mirror_manager = MirrorManager::new(mirrors_path, CacheStrategy::RespectTtl).await;
        if let Err(net_err) = mirror_manager {
            tracing::error!("MirrorManager initialization failed: {net_err}");
            return Err(ZvError::NetworkError(net_err));
        };
        Ok(Self {
            base_path: zv_base_path.as_ref().to_path_buf(),
            client,
            mirror_manager: mirror_manager.expect("valid mirror manager"),
        })
    }
}

impl ZvNetwork {
    /// Download a file with comprehensive timeout handling and retries
    pub async fn download_file_with_retry<U, P>(
        &self,
        url: U,
        destination: P,
        tarball: &str,
    ) -> Result<(), NetErr>
    where
        U: AsRef<str> + std::fmt::Display + std::fmt::Debug,
        P: AsRef<Path>,
    {
        const MAX_RETRIES: usize = 3;
        const CHUNK_TIMEOUT: Duration = Duration::from_secs(15); // 15s per chunk
        const STALL_TIMEOUT: Duration = Duration::from_secs(30); // 30s without progress

        let filename = tarball;

        for attempt in 1..=MAX_RETRIES {
            match self
                .try_download_with_timeout(
                    &url,
                    &destination,
                    filename,
                    CHUNK_TIMEOUT,
                    STALL_TIMEOUT,
                )
                .await
            {
                ok @ Ok(()) => return ok,
                Err(e) if attempt == MAX_RETRIES => {
                    tracing::error!("Download failed after {} attempts: {}", MAX_RETRIES, e);
                    return Err(e);
                }
                Err(e) => {
                    tracing::warn!("Download attempt {} failed, retrying: {}", attempt, e);
                    tokio::time::sleep(Duration::from_millis(1000 * attempt as u64)).await;
                }
            }
        }

        unreachable!()
    }

    async fn try_download_with_timeout<U, P>(
        &self,
        url: U,
        destination: P,
        filename: &str,
        chunk_timeout: Duration,
        stall_timeout: Duration,
    ) -> Result<(), NetErr>
    where
        U: AsRef<str> + std::fmt::Display + std::fmt::Debug,
        P: AsRef<Path>,
    {
        // Get content length for progress bar (with timeout)
        let content_length = None;

        // Create progress bar
        let pb = if let Some(size) = content_length {
            let pb = ProgressBar::new(size);
            pb.set_style(ProgressStyle::default_bar()
                .template("{msg} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("█ ▓ ░"));
            pb.set_message(format!("Downloading {}", filename));
            pb
        } else {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("{msg} {spinner:.green}")
                    .unwrap(),
            );
            pb.set_message(format!("Downloading {}", filename));
            pb
        };

        // Start the request with timeout
        let res = tokio::time::timeout(
            Duration::from_secs(30),
            self.client.get(url.as_ref()).send(),
        )
        .await
        .map_err(|_| eyre!("Request timed out after 30 seconds"))?
        .map_err(|e| eyre!("Failed to send request: {}", e))?;

        if !res.status().is_success() {
            return Err(NetErr::HTTP(res.status()));
        }

        // Create file
        let mut file = tokio::fs::File::create(destination.as_ref())
            .await
            .map_err(NetErr::FileIo)
            .context("Failed to create file")?;

        let mut stream = res.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut last_progress_time = Instant::now();

        // Download with per-chunk timeout and stall detection
        loop {
            let chunk_result = tokio::time::timeout(chunk_timeout, stream.next()).await;

            match chunk_result {
                Ok(Some(chunk_result)) => {
                    let chunk = chunk_result
                        .map_err(NetErr::Reqwest)
                        .context("Failed to download chunk")?;

                    file.write_all(&chunk)
                        .await
                        .map_err(NetErr::FileIo)
                        .context("Failed to write chunk to file")?;

                    downloaded += chunk.len() as u64;
                    pb.set_position(downloaded);
                    last_progress_time = Instant::now();
                }
                Ok(None) => {
                    // Stream ended normally
                    break;
                }
                Err(_) => {
                    // Chunk timeout
                    return Err(NetErr::Timeout(format!(
                        "Chunk download timed out after {:?}",
                        chunk_timeout
                    )));
                }
            }

            // Check for stall (no progress for too long)
            if last_progress_time.elapsed() > stall_timeout {
                return Err(NetErr::Stalled {
                    duration: stall_timeout,
                });
            }
        }

        // Ensure file is flushed
        file.flush().await.map_err(NetErr::FileIo)?;
        drop(file); // Explicit close

        pb.finish_and_clear();
        println!("✓ Downloaded {}", Paint::green(filename));

        tracing::debug!(
            "Download completed: {} -> {} ({} bytes)",
            url,
            destination.as_ref().display(),
            downloaded
        );

        Ok(())
    }

    /// Original download method for backwards compatibility
    pub async fn download_file<U, P>(&self, url: U, destination: P) -> Result<(), NetErr>
    where
        U: AsRef<str> + std::fmt::Display + std::fmt::Debug,
        P: AsRef<Path>,
    {
        // Use the robust version by default
        self.download_file_with_retry(url, destination, "zig-tarball-placeholder")
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::{app::utils::zig_tarball, init_tracing};

    use super::*;
    use color_eyre::eyre::bail;
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;
    use tempfile::NamedTempFile;

    #[tokio::test]
    #[tracing::instrument]
    async fn test_mirror_with_timeout_handling() -> color_eyre::Result<()> {
        color_eyre::install()?;
        init_tracing()?;

        let network = ZvNetwork::default();
        let version = semver::Version::parse("0.14.1").unwrap();

        // Test problematic mirror
        let temp_file = NamedTempFile::new()?;
        let destination = temp_file.path().to_path_buf();

        let url = get_url(&version);
        tracing::info!("Testing timeout-prone mirror: {}", url);

        let start_time = std::time::Instant::now();

        // Use robust download with overall timeout
        let download_result = tokio::time::timeout(
            Duration::from_secs(300), // 5 minute overall timeout
            network.download_file_with_retry(
                &url,
                &destination,
                zig_tarball(&ZigVersion::from(version), None)
                    .unwrap()
                    .as_str(),
            ),
        )
        .await;

        match download_result {
            Ok(Ok(())) => {
                let elapsed = start_time.elapsed();
                let size = tokio::fs::metadata(&destination).await?.len();
                tracing::info!(
                    "✓ Robust download succeeded in {:.2}s ({} MB)",
                    elapsed.as_secs_f64(),
                    size as f64 / 1024.0 / 1024.0
                );
            }
            Ok(Err(e)) => {
                tracing::error!("✗ Robust download failed: {}", e);
                return Err(e.into());
            }
            Err(_) => {
                tracing::error!("✗ Overall download timeout (5 minutes)");
                return Err(eyre!("Download took longer than 5 minutes"));
            }
        }

        temp_file.close()?;
        Ok(())
    }

    fn get_url(version: &semver::Version) -> String {
        use rand::prelude::IndexedRandom;

        let zig_urls: Vec<&'static str> = vec![
            "https://pkg.machengine.org/zig",
            "https://zigmirror.hryx.net/zig",
            "https://zig.linus.dev/zig",
            "https://zig.squirl.dev",
            "https://zig.florent.dev",
            "https://zig.mirror.mschae23.de/zig",
            "https://zigmirror.meox.dev",
        ];
        let tarball: String = zig_tarball(&ZigVersion::from(version), None).unwrap();

        let mut rng = rand::thread_rng();
        if let Some(url) = zig_urls.choose(&mut rng) {
            // Try with version in path (like official site)
            format!("{url}/{version}/{tarball}?source=zv-is-cooking")
        } else {
            format!("https://ziglang.org/download/{version}/{tarball}")
        }
    }
}
