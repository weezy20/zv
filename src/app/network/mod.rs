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

/// Cache strategy for index loading
#[derive(Debug, Clone, Copy)]
pub enum CacheStrategy {
    /// Always fetch fresh data from network
    AlwaysRefresh,
    /// Use cached data if available, only fetch if no cache exists
    PreferCache,
    /// Respect TTL - use cache if not expired, otherwise refresh
    RespectTtl,
    /// Only load from cache, no network request
    OnlyCache,
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
    mirror_manager: Option<MirrorManager>,
    /// Zig version index
    index_manager: IndexManager,
    /// ZV_DIR
    base_path: PathBuf,
    /// Download cache path (ZV_DIR/downloads)
    download_cache: PathBuf,
    /// Network Client
    client: reqwest::Client,
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
        let client = create_client()?;

        Ok(Self {
            download_cache: zv_base_path.as_ref().join("downloads"),
            index_manager: IndexManager::new(
                zv_base_path.as_ref().join("index.toml"),
                client.clone(),
            ),
            client,
            toolchain_manager,
            base_path: zv_base_path.as_ref().to_path_buf(),
            mirror_manager: None,
        })
    }
    /// Load the mirror manager if not already done
    pub async fn ensure_mirror_manager(&mut self) -> Result<&mut MirrorManager, ZvError> {
        if !self.download_cache.is_dir() {
            tokio::fs::create_dir_all(&self.download_cache)
                .await
                .map_err(ZvError::Io)
                .wrap_err("Creation of download cache directory failed")?;
        }
        if self.mirror_manager.is_none() {
            let mirrors_path = self.base_path.join("mirrors.toml");
            let mirror_manager = MirrorManager::init_and_load(
                mirrors_path,
                CacheStrategy::RespectTtl,
            )
            .await
            .map_err(|net_err| {
                tracing::error!(target: TARGET, "MirrorManager initialization failed: {net_err}");
                ZvError::NetworkError(net_err)
            })?;
            self.mirror_manager = Some(mirror_manager);
            tracing::info!(target: TARGET, "Loaded {} community mirrors", self.mirror_manager.as_mut().unwrap().mirrors().await.unwrap_or(&[]).len());
        }
        Ok(self.mirror_manager.as_mut().unwrap())
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
        // Load index with cache strategy
        self.index_manager.ensure_loaded(cache_strategy).await?;

        // Get the index and retrieve latest stable version
        let index = self.index_manager.get_index().unwrap(); // Safe unwrap after ensure_loaded

        match index.get_latest_stable() {
            Some(stable_version) => Ok(stable_version),
            None => Err(eyre!("No stable version found in Zig download index").into()),
        }
    }

    /// Fetch the latest master as a [ZigVersion::Semver] using smart optimizations
    pub async fn fetch_master_version(&mut self) -> Result<ZigVersion, ZvError> {
        match try_partial_fetch(&self.client).await {
            Ok(fetched_version) => Ok(fetched_version),
            Err(partial_err) => {
                tracing::error!(target: "zv::network::fetch_master_version", "Partial fetch failed: {}", partial_err);

                match self
                    .index_manager
                    .ensure_loaded(CacheStrategy::AlwaysRefresh)
                    .await
                {
                    Ok(_) => {
                        let index = self.index_manager.get_index().unwrap();

                        if let Some(master_release) = index.get_master_version() {
                            Ok(master_release)
                        } else {
                            Err(eyre!("No master version found in index").into())
                        }
                    }
                    Err(e) => {
                        tracing::error!(target: "zv::network::fetch_master_version", "Failed to get current master version from network: {e}. Falling back to cached index");
                        match self
                            .index_manager
                            .ensure_loaded(CacheStrategy::OnlyCache)
                            .await
                        {
                            Ok(_) => {
                                let index = self.index_manager.get_index().unwrap();
                                return index.get_master_version().ok_or_else(|| {
                                    eyre!("No master version found in index").into()
                                });
                            }
                            Err(err) => {
                                tracing::error!(
                                    target: "zv::network::fetch_master_version",
                                    "Cache read failed. Cannot determine master version"
                                );
                                return Err(err);
                            }
                        }
                        Err(e)
                    }
                }
            }
        }
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

async fn try_partial_fetch(client: &reqwest::Client) -> Result<ZigVersion, PartialFetchError> {
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

pub(crate) fn create_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(zv_agent())
        .pool_max_idle_per_host(0) // Don't keep idle connections
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(*NETWORK_TIMEOUT_SECS))
        .build()
        .map_err(|e| ZvError::NetworkError(NetErr::Reqwest(e)))
        .wrap_err("Failed to build HTTP client")
}
