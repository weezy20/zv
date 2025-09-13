use crate::app::constants::ZIG_DOWNLOAD_INDEX_JSON;
use crate::app::toolchain::ToolchainManager;
use crate::app::utils::zv_agent;
use crate::{NetErr, ZigVersion, ZvError, tools};
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Url;
use std::sync::{Arc, LazyLock};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use yansi::Paint;
mod mirror;
use mirror::*;
mod zig_index;
use zig_index::*;

/// Result of fetching master version with optimization hints
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// The determined master version
    pub version: ZigVersion,
    /// Whether the index needs to be updated due to version mismatch or missing data
    pub index_needs_update: bool,
}

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
/// 21 days default TTL for index
pub static INDEX_TTL_DAYS: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("ZV_INDEX_TTL_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21)
});
/// 21 days default TTL for mirrors list
pub static MIRRORS_TTL_DAYS: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("ZV_MIRRORS_TTL_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(21)
});

#[derive(Debug, Clone)]
pub struct ZvNetwork {
    /// Management layer for community-mirrors
    mirror_manager: MirrorManager,
    /// Zig version index
    index_manager: IndexManager,
    /// ZV_DIR
    base_path: PathBuf,
    /// Download cache path (ZV_DIR/downloads)
    download_cache: PathBuf,
    /// Network Client
    client: Arc<reqwest::Client>,
    /// Client with retry logic
    retry_client: Arc<reqwest_middleware::ClientWithMiddleware>,
    /// Reference to ToolchainManager for version management
    toolchain_manager: Arc<ToolchainManager>,
}

// === Initialize ZvNetwork ===
impl ZvNetwork {
    /// Initialize ZvNetwork with given base path (ZV_DIR)
    pub async fn new(
        zv_base_path: impl AsRef<Path>,
        toolchain_manager: Arc<ToolchainManager>,
    ) -> Result<Self, ZvError> {
        use reqwest_middleware::ClientBuilder;
        use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

        let base_client = reqwest::Client::builder()
            .user_agent(zv_agent())
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(NetErr::Reqwest)
            .wrap_err("Failed to build HTTP client")?;

        let client = Arc::new(base_client.clone());

        let retry_policy = ExponentialBackoff::builder().build_with_max_retries(3);
        let retry_client = Arc::new(
            ClientBuilder::new(base_client)
                .with(RetryTransientMiddleware::new_with_policy(retry_policy))
                .build(),
        );

        if !zv_base_path.as_ref().join("downloads").is_dir() {
            tokio::fs::create_dir_all(zv_base_path.as_ref().join("downloads"))
                .await
                .map_err(|io_err| {
                    tracing::error!(target: TARGET, "Failed to create \"downloads\" directory: {io_err}");
                    ZvError::Io(io_err)
                })?;
        }
        let mirrors_path = zv_base_path.as_ref().join("mirrors.toml");
        let mirror_manager = MirrorManager::init_and_load(
            mirrors_path,
            CacheStrategy::RespectTtl,
            Arc::clone(&client),
            Arc::clone(&retry_client),
        )
        .await
        .map_err(|net_err| {
            tracing::error!(target: TARGET, "MirrorManager initialization failed: {net_err}");
            ZvError::NetworkError(net_err)
        })?;
        Ok(Self {
            download_cache: zv_base_path.as_ref().join("downloads"),
            index_manager: IndexManager::new(
                zv_base_path.as_ref().join("index.toml"),
                Arc::clone(&client),
            ),
            client,
            toolchain_manager,
            retry_client,
            base_path: zv_base_path.as_ref().to_path_buf(),
            mirror_manager,
        })
    }
    /// Ensure download cache directory exists (i.e. ZV_DIR/downloads)
    async fn ensure_download_cache(&self) -> Result<(), ZvError> {
        if !self.download_cache.is_dir() {
            tokio::fs::create_dir_all(&self.download_cache)
                .await
                .map_err(ZvError::Io)
                .wrap_err("Creation of download cache directory failed")?;
        }
        Ok(())
    }
    fn versions_path(&self) -> PathBuf {
        self.base_path.join("versions")
    }
    fn index_path(&self) -> PathBuf {
        self.base_path.join("index.toml")
    }
    fn mirrors_path(&self) -> PathBuf {
        self.base_path.join("mirrors.toml")
    }
}

// === Usage ===
impl ZvNetwork {
    pub fn download_version() {
        todo!("Implement version download logic")
    }

    /// Fetch the latest stable version using prefer-cache strategy
    /// Returns the latest stable version from the Zig download index
    pub async fn fetch_stable_version(&mut self) -> Result<ZigVersion, ZvError> {
        // Ensure download cache directory exists
        self.ensure_download_cache().await.map_err(|e| {
            tracing::warn!(target: TARGET, "Failed to ensure download cache: {e}");
            e
        })?;

        // Load index with preferred cache strategy
        self.index_manager
            .ensure_loaded(CacheStrategy::PreferCache)
            .await?;

        // Get the index and retrieve latest stable version
        let index = self.index_manager.get_index().unwrap(); // Safe unwrap after ensure_loaded

        match index.get_latest_stable() {
            Some(stable_version) => {
                tracing::info!(target: TARGET, "Found latest stable version: {}", stable_version);
                Ok(stable_version)
            }
            None => {
                tracing::error!(target: TARGET, "No stable version found in index");
                Err(eyre!("No stable version found in Zig download index").into())
            }
        }
    }

    /// Fetch the latest master as a [ZigVersion::Semver] using smart optimizations
    pub async fn fetch_master_version(&mut self) -> Result<ZigVersion, ZvError> {
        match try_partial_fetch(self.client.clone()).await {
            Ok(fetched_version) => {
                println!(
                    "{} Fetched master version via partial fetch: {}",
                    Paint::blue("Info:").bold(),
                    Paint::green(&fetched_version.to_string()).bold()
                );
                return Ok(fetched_version);
            }
            Err(err) => {
                // Partial fetch failed - fall back to full index fetch
                tracing::warn!(target: TARGET, "Partial fetch failed: {err}, falling back to full fetch");
                self.index_manager
                    .ensure_loaded(CacheStrategy::AlwaysRefresh)
                    .await?;
                let index = self.index_manager.get_index().unwrap(); // Safe unwrap after ensure_loaded

                if let Some(master_release) = index.get_master_version() {
                    Ok(master_release)
                } else {
                    Err(eyre!("No master version found in index").into())
                }
            }
        }
    }
}

/// Attempts to fetch just the beginning of the JSON file to get master version
async fn try_partial_fetch(client: Arc<reqwest::Client>) -> Result<ZigVersion> {
    // Try to get first 1024 bytes - should be enough for master version
    let response = client
        .get(ZIG_DOWNLOAD_INDEX_JSON)
        .header("Range", "bytes=0-88")
        .send()
        .await?;

    if response.status() == 206 {
        // Partial Content
        let partial_text = response.text().await?;
        let v = parse_master_version_fast(&partial_text)?;
        return Ok(ZigVersion::Semver(semver::Version::parse(&v).map_err(|e| {
            tracing::error!(target: TARGET, "Failed to parse master version from partial fetch: {e}");
            eyre!(e)
        })?));
    } else {
        bail!("Server did not support partial content")
    }
}

/// Ultra-fast string parsing approach - for partial JSON content  
fn parse_master_version_fast(json_text: &str) -> Result<String> {
    // Look for: "master": { "version": "..."
    if let Some(master_start) = json_text.find(r#""master""#) {
        let search_area = &json_text[master_start..];

        // Find the version field within the master object
        if let Some(version_start) = search_area.find(r#""version""#) {
            let after_version = &search_area[version_start + 9..]; // Skip past "version" which is 9 chars including quotes

            // Find the colon and opening quote
            if let Some(colon_pos) = after_version.find(':') {
                let after_colon = &after_version[colon_pos + 1..];
                if let Some(quote_start) = after_colon.find('"') {
                    let version_content = &after_colon[quote_start + 1..];

                    // Find closing quote
                    if let Some(quote_end) = version_content.find('"') {
                        let version = &version_content[..quote_end];
                        return Ok(version.to_string());
                    }
                }
            }
        }
    }

    bail!("Could not extract master version from partial JSON")
}
