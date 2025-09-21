use crate::app::NETWORK_TIMEOUT_SECS;
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
pub use zig_index::*;

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
    /// Download a given zig_tarball URL to the download cache, returns the path to the downloaded file
    pub async fn download_version(
        &mut self,
        zig_tarball: &str,
        download_artifact: &ArtifactInfo,
    ) -> Result<PathBuf, ZvError> {
        Err(ZvError::General(eyre!(
            "Direct download of {} not implemented yet, use mirrors",
            zig_tarball
        )))
    }

    /// Checks if the given version is valid by checking it against the index
    pub async fn validate_semver(
        &mut self,
        version: &semver::Version,
    ) -> Result<ZigRelease, ZvError> {
        // Try to load index with TTL respect, fallback to cache on network failure
        match self
            .index_manager
            .ensure_loaded(CacheStrategy::RespectTtl)
            .await
        {
            Ok(index) => index.contains_version(version).cloned().ok_or_else(|| {
                ZvError::ZigNotFound(eyre!("Version {} not found in Zig download index", version))
            }),
            Err(network_err) => {
                tracing::error!(
                    target: "zv::network::validate_semver",
                    "Failed to load index from network: {network_err}. Falling back to cached index"
                );

                // Fallback to cache
                match self
                    .index_manager
                    .ensure_loaded(CacheStrategy::OnlyCache)
                    .await
                {
                    Ok(index) => index.contains_version(version).cloned().ok_or_else(|| {
                        ZvError::ZigNotFound(eyre!(
                            "Version {} not found in cached Zig download index",
                            version
                        ))
                    }),
                    Err(cache_err) => {
                        tracing::error!(
                            target: "zv::network::validate_semver",
                            "Cache read failed. Cannot validate version"
                        );
                        Err(cache_err)
                    }
                }
            }
        }
    }
    pub async fn fetch_master_version(&mut self) -> Result<ZigRelease, ZvError> {
        // Try enhanced partial fetch first
        match try_partial_fetch_master(&self.client).await {
            Ok(PartialFetchResult::Complete(complete_release)) => {
                tracing::debug!(
                    target: "zv::network::fetch_master_version",
                    "Got complete master ZigRelease from partial fetch"
                );
                return Ok(complete_release);
            }
            Ok(PartialFetchResult::VersionOnly(partial_master_version)) => {
                tracing::debug!(
                    target: "zv::network::fetch_master_version",
                    "Got version from partial fetch: {partial_master_version}, checking against cache"
                );

                // Check if we have this version in cache
                if let Ok(index) = self
                    .index_manager
                    .ensure_loaded(CacheStrategy::RespectTtl)
                    .await
                {
                    if let Some(cached_master) =
                        index.get_master_version().and_then(|cached_master| {
                            semver::Version::parse(&cached_master.version_string())
                                .ok()
                                .filter(|cached_version| *cached_version == partial_master_version)
                                .map(|_| cached_master.clone())
                        })
                    {
                        tracing::debug!(
                            target: "zv::network::fetch_master_version",
                            "Partial fetch version matches cached version, using cache"
                        );
                        return Ok(cached_master);
                    }
                }

                tracing::debug!(
                    target: "zv::network::fetch_master_version",
                    "Version mismatch or no cache, forcing full refresh"
                );
            }
            Err(err) => {
                tracing::debug!(
                    target: "zv::network::fetch_master_version",
                    "Partial fetch failed: {err}, falling back to full fetch"
                );
            }
        }

        // Fallback to full index fetch with cache fallback on failure
        match self
            .index_manager
            .ensure_loaded(CacheStrategy::AlwaysRefresh)
            .await
        {
            Ok(index) => index.get_master_version().cloned().ok_or_else(|| {
                ZvError::ZigVersionResolveError(eyre!(
                    "No master version found in Zig download index after full refresh"
                ))
            }),
            Err(network_err) => {
                tracing::error!(
                    target: "zv::network::fetch_master_version",
                    "Failed to refresh index: {network_err}. Falling back to cached index"
                );

                // Fallback to cache
                match self
                    .index_manager
                    .ensure_loaded(CacheStrategy::OnlyCache)
                    .await
                {
                    Ok(index) => index.get_master_version().cloned().ok_or_else(|| {
                        ZvError::ZigVersionResolveError(eyre!(
                            "No master version found in cached index"
                        ))
                    }),
                    Err(cache_err) => {
                        tracing::error!(
                            target: "zv::network::fetch_master_version",
                            "Cache read failed. Cannot determine master version"
                        );
                        Err(cache_err)
                    }
                }
            }
        }
    }

    /// Returns the latest stable version from the Zig download index with consistent error handling
    pub async fn fetch_latest_stable_version(
        &mut self,
        cache_strategy: CacheStrategy,
    ) -> Result<ZigRelease, ZvError> {
        match cache_strategy {
            CacheStrategy::AlwaysRefresh | CacheStrategy::RespectTtl => {
                // Try the requested strategy first, fallback to cache on network failure
                match self.index_manager.ensure_loaded(cache_strategy).await {
                    Ok(index) => index.get_latest_stable_release().cloned().ok_or_else(|| {
                        ZvError::ZigVersionResolveError(eyre!(
                            "No stable version found in Zig download index"
                        ))
                    }),
                    Err(network_err) => {
                        tracing::error!(
                            target: "zv::network::fetch_latest_stable_version",
                            "Failed to get latest stable version from network: {network_err}. Falling back to cached index"
                        );

                        // Fallback to cache
                        match self
                            .index_manager
                            .ensure_loaded(CacheStrategy::OnlyCache)
                            .await
                        {
                            Ok(index) => index.get_latest_stable_release().cloned().ok_or_else(|| {
                                ZvError::ZigVersionResolveError(eyre!(
                                    "No stable version found in cached index"
                                ))
                            }),
                            Err(cache_err) => {
                                tracing::error!(
                                    target: "zv::network::fetch_latest_stable_version",
                                    "Cache read failed. Cannot determine latest stable version"
                                );
                                Err(cache_err)
                            }
                        }
                    }
                }
            }
            CacheStrategy::PreferCache | CacheStrategy::OnlyCache => {
                let index = self.index_manager.ensure_loaded(cache_strategy).await?;

                index.get_latest_stable_release().cloned().ok_or_else(|| {
                    ZvError::ZigVersionResolveError(eyre!(
                        "No stable version found in Zig download index"
                    ))
                })
            }
        }
    }
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

#[derive(thiserror::Error, Debug)]
pub(crate) enum PartialFetchError {
    #[error("Failed to parse partial JSON: {0}")]
    Parse(color_eyre::Report),
    #[error("Network error: {0}")]
    Network(reqwest::Error),
    #[error("Timeout error: {0}")]
    Timeout(reqwest::Error),
    #[error("Unexpected status code: {0}")]
    Not206(reqwest::StatusCode),
}

#[derive(Debug, Clone)]
pub(crate) enum PartialFetchResult {
    /// We got the complete ZigRelease for master
    Complete(ZigRelease),
    /// We only got the version string, need full fetch
    VersionOnly(semver::Version),
}

pub(crate) async fn try_partial_fetch_master(
    client: &reqwest::Client,
) -> Result<PartialFetchResult, PartialFetchError> {
    // (8KB) to increase chances of getting complete master object
    let response = client
        .get(ZIG_DOWNLOAD_INDEX_JSON)
        .header("Range", "bytes=0-8191") // 8KB should be enough for most master objects
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
        let partial_text = response.text().await.map_err(PartialFetchError::Network)?;

        // First try to extract complete master ZigRelease
        match try_extract_complete_master(&partial_text) {
            Ok(complete_release) => {
                tracing::debug!(
                    target: "zv::network::partial_fetch",
                    "Successfully extracted complete master ZigRelease from partial fetch"
                );
                return Ok(PartialFetchResult::Complete(complete_release));
            }
            Err(e) => {
                tracing::debug!(
                    target: "zv::network::partial_fetch",
                    "Could not extract complete master object: {e}, falling back to version-only parsing"
                );
            }
        }

        // Fallback to version-only extraction
        let version_str =
            parse_master_version_fast(&partial_text).map_err(PartialFetchError::Parse)?;
        let version =
            semver::Version::parse(&version_str).map_err(|e| PartialFetchError::Parse(e.into()))?;

        Ok(PartialFetchResult::VersionOnly(version))
    } else {
        Err(PartialFetchError::Not206(response.status()))
    }
}

/// Attempts to extract a complete master ZigRelease from partial JSON
/// This works by finding the master object boundaries and trying to parse it
fn try_extract_complete_master(json_text: &str) -> Result<ZigRelease> {
    // Find the start of the master object
    let master_start = json_text
        .find(r#""master":"#)
        .ok_or_else(|| eyre!("Could not find master key in partial JSON (length: {})", json_text.len()))?;

    // Find the opening brace after "master":
    let after_master_key = &json_text[master_start + 8..]; // Skip past "master"
    let colon_pos = after_master_key
        .find(':')
        .ok_or_else(|| eyre!("Could not find colon after master key"))?;

    let after_colon = &after_master_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('{') {
        return Err(eyre!("Master value is not an object"));
    }

    // Now we need to find the complete master object by counting braces
    let mut brace_count = 0;
    let mut in_string = false;
    let mut escape_next = false;
    let mut end_pos = None;

    for (i, ch) in after_colon.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => {
                brace_count += 1;
                if brace_count == 1 && i == 0 {
                    continue; // This is our opening brace
                }
            }
            '}' if !in_string => {
                brace_count -= 1;
                if brace_count == 0 {
                    end_pos = Some(i + 1); // Include the closing brace
                    break;
                }
            }
            _ => {}
        }
    }

    let end_pos = end_pos.ok_or_else(|| {
        eyre!(
            "Could not find end of master object (brace_count: {}, partial_length: {}, in_string: {})", 
            brace_count, 
            after_colon.len(),
            in_string
        )
    })?;
    let master_json = &after_colon[..end_pos];

    // Try to parse the extracted JSON as a NetworkZigRelease and convert to ZigRelease
    use crate::app::network::zig_index::models::{NetworkZigRelease, ZigRelease, ZigIndex, ArtifactInfo};
    use crate::types::{TargetTriple, ResolvedZigVersion};
    use std::collections::{HashMap, BTreeMap};
    
    let network_release: NetworkZigRelease = serde_json::from_str(master_json)
        .map_err(|e| eyre!("Failed to parse extracted master JSON (length: {}): {e}", master_json.len()))?;
    
    // Convert to ZigRelease
    let resolved_version = if let Some(version_str) = &network_release.version {
        match semver::Version::parse(version_str) {
            Ok(version) => ResolvedZigVersion::MasterVersion(version),
            Err(_) => {
                tracing::warn!("Failed to parse master version: {}", version_str);
                ResolvedZigVersion::Master
            }
        }
    } else {
        ResolvedZigVersion::Master
    };

    // Convert network artifacts to runtime artifacts
    let mut runtime_artifacts = HashMap::new();
    for (target_key, network_artifact) in network_release.targets.into_iter() {
        if let Some(target_triple) = TargetTriple::from_key(&target_key) {
            let artifact_info = ArtifactInfo {
                ziglang_org_tarball: network_artifact.ziglang_org_tarball,
                shasum: network_artifact.shasum,
                size: network_artifact.size,
            };
            runtime_artifacts.insert(target_triple, artifact_info);
        } else {
            tracing::warn!("Failed to parse target key: {}", target_key);
        }
    }

    let master_release = ZigRelease::new(
        resolved_version,
        network_release.date,
        runtime_artifacts,
    );

    Ok(master_release)
}

/// Ultra-fast string parsing approach - for partial JSON content when complete extraction fails
pub(crate) fn parse_master_version_fast(json_text: &str) -> Result<String> {
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

    Err(eyre!("Could not extract master version from partial JSON"))
}
