use crate::app::constants::ZIG_DOWNLOAD_INDEX_JSON;
use crate::app::toolchain::ToolchainManager;
use crate::app::utils::{ProgressHandle, zv_agent};
use crate::{NetErr, ZigVersion, ZvError, tools};
use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use futures::StreamExt;
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
/// Network timeout in seconds for operations
pub static NETWORK_TIMEOUT_SECS: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("ZV_NETWORK_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15)
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
            .pool_max_idle_per_host(0) // Don't keep idle connections
            .connect_timeout(Duration::from_secs(5.min(*NETWORK_TIMEOUT_SECS)))
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

    /// Returns the latest stable version from the Zig download index. Network request is controlled by [CacheStrategy].
    pub async fn fetch_last_stable_version(
        &mut self,
        cache_strategy: CacheStrategy,
    ) -> Result<ZigVersion, ZvError> {
        // Create a spinner progress bar
        let spinner = ProgressHandle::spawn();
        spinner.start("Loading zig index...").await;
        // Load index with cache strategy
        self.index_manager.ensure_loaded(cache_strategy).await?;

        // Get the index and retrieve latest stable version
        let index = self.index_manager.get_index().unwrap(); // Safe unwrap after ensure_loaded

        match index.get_latest_stable() {
            Some(stable_version) => {
                spinner
                    .finish(format!(
                        "✓ Found latest stable version from index: {}",
                        Paint::green(&stable_version).bold()
                    ))
                    .await;
                Ok(stable_version)
            }
            None => {
                spinner.finish_with_error("✗ No stable version found in index");
                Err(eyre!("No stable version found in Zig download index").into())
            }
        }
    }

    /// Fetch the latest master as a [ZigVersion::Semver] using smart optimizations
    pub async fn fetch_master_version(&mut self) -> Result<ZigVersion, ZvError> {
        let progress = ProgressHandle::spawn();

        progress
            .start(Paint::blue("Fetching master version...").bold().to_string())
            .await;
        let result = match try_partial_fetch(self.client.clone()).await {
            Ok(fetched_version) => {
                progress
                    .update("✓ Fetched master version via partial fetch")
                    .await;
                progress
                    .finish(&format!(
                        "✓ Fetched Master Version: {}",
                        Paint::green(&fetched_version.to_string()).bold()
                    ))
                    .await;

                Ok(fetched_version)
            }
            Err(partial_err) => {
                if let PartialFetchError::Timeout(err) = partial_err {
                    tracing::error!(target: "zv::network::fetch_master_version", "Partial fetch timed out: {}", err);
                    progress.finish_with_error(Paint::red("Unable to reach network").to_string()).await;
                    return Err(ZvError::NetworkError(NetErr::Reqwest(err)));
                }
                tracing::error!(target: "zv::network::fetch_master_version", "Partial fetch failed: {:?}", partial_err);
                progress
                    .update("Partial fetch failed, falling back to full index fetch")
                    .await;

                match self
                    .index_manager
                    .ensure_loaded(CacheStrategy::AlwaysRefresh)
                    .await
                {
                    Ok(_) => {
                        let index = self.index_manager.get_index().unwrap();

                        if let Some(master_release) = index.get_master_version() {
                            progress
                                .finish(&format!(
                                    "✓ Fetched Master Version: {}",
                                    Paint::green(&master_release.to_string()).bold()
                                ))
                                .await;
                            Ok(master_release)
                        } else {
                            progress
                                .finish_with_error(
                                    "✗ Failed to fetch master version from updated index.",
                                )
                                .await;
                            Err(eyre!("No master version found in index").into())
                        }
                    }
                    Err(e) => {
                        progress
                            .finish_with_error(format!(
                                "✗ Failed to fetch master version due to {}",
                                e.to_string()
                            ))
                            .await;
                        Err(e)
                    }
                }
            }
        };
        result
    }
}

#[derive(thiserror::Error, Debug)]
enum PartialFetchError {
    #[error("Failed to parse partial JSON: {0}")]
    Parse(color_eyre::Report),
    #[error("Network error: {0}")]
    Network(reqwest::Error),
    #[error("Timeout error: {0}")]
    Timeout(reqwest::Error),
    #[error("Unexpected status code: {0}")]
    Not206(reqwest::StatusCode),
}

async fn try_partial_fetch(client: Arc<reqwest::Client>) -> Result<ZigVersion, PartialFetchError> {
    let response = client
        .get(ZIG_DOWNLOAD_INDEX_JSON)
        .header("Range", "bytes=0-88")
        .timeout(Duration::from_secs(*NETWORK_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|err| {
            if err.is_timeout() {
                PartialFetchError::Timeout(err)
            } else {
                PartialFetchError::Network(err)
            }
        })?;

    if response.status() == 206 {
        // Partial Content
        let partial_text = response.text().await.map_err(PartialFetchError::Network)?;
        let v =
            parse_master_version_fast(&partial_text).map_err(|e| PartialFetchError::Parse(e))?; // Assume parse fail means unsupported
        let version = semver::Version::parse(&v).map_err(|e| PartialFetchError::Parse(e.into()))?;
        Ok(ZigVersion::Semver(version))
    } else {
        // Server responded but didn't give partial content
        Err(PartialFetchError::Not206(response.status()))
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
