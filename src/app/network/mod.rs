use crate::app::constants::ZIG_DOWNLOAD_INDEX_JSON;
use crate::app::utils::{ProgressHandle, remove_files, verify_checksum, zv_agent};
use crate::{NetErr, ZvError};
use color_eyre::eyre::{Result, WrapErr, eyre};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crate::types::{ResolvedZigVersion, TargetTriple};
use std::collections::HashMap;
mod mirror;
use mirror::*;
mod zig_index;
pub use zig_index::*;
mod download;
use download::{move_to_final_location, stream_download_file};
pub use {ArtifactInfo, NetworkZigRelease, ZigRelease};
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

/// Result of a successful download operation containing paths to verified files and mirror information
#[derive(Debug, Clone)]
pub struct ZigDownload {
    /// Path to the verified tarball in downloads folder
    pub tarball_path: PathBuf,
    /// Path to the minisign signature file
    pub minisig_path: PathBuf,
    /// Mirror that was successfully used for the download
    pub mirror_used: String,
}

#[derive(Debug, Clone)]
pub struct ZvNetwork {
    /// Management layer for community-mirrors
    pub mirror_manager: Option<MirrorManager>,
    /// Zig version index
    pub index_manager: IndexManager,
    /// ZV_DIR
    base_path: PathBuf,
    /// Download cache path (ZV_DIR/downloads)
    download_cache: PathBuf,
    /// Network Client
    client: reqwest::Client,
}

// === Initialize ZvNetwork ===
impl ZvNetwork {
    /// Initialize ZvNetwork with given base path (ZV_DIR)
    pub async fn new(
        zv_base_path: impl AsRef<Path>,
        download_cache: PathBuf,
    ) -> Result<Self, ZvError> {
        let client = create_client()?;

        Ok(Self {
            download_cache,
            index_manager: IndexManager::new(
                zv_base_path.as_ref().join("index.toml"),
                client.clone(),
            ),
            client,
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
            tracing::trace!(target: TARGET, "Loaded {} community mirrors", self.mirror_manager.as_mut().unwrap().all_mirrors_mut().await.unwrap_or(&mut []).len());
        }
        Ok(self.mirror_manager.as_mut().unwrap())
    }

    /// Force refresh the Zig index from network
    pub async fn sync_zig_index(&mut self) -> Result<(), ZvError> {
        self.index_manager
            .ensure_loaded(CacheStrategy::AlwaysRefresh)
            .await?;
        Ok(())
    }

    /// Force refresh the community mirrors list from network
    pub async fn sync_mirrors(&mut self) -> Result<usize, ZvError> {
        self.ensure_mirror_manager().await?;

        if let Some(mirror_manager) = self.mirror_manager.as_mut() {
            mirror_manager
                .load_mirrors(CacheStrategy::AlwaysRefresh)
                .await
                .map_err(ZvError::NetworkError)?;

            let mirror_count = mirror_manager
                .all_mirrors_mut()
                .await
                .map(|mirrors| mirrors.len())
                .unwrap_or(0);

            return Ok(mirror_count);
        }

        Ok(0)
    }
    #[allow(unused)]
    pub fn versions_path(&self) -> PathBuf {
        self.base_path.join("versions")
    }
    #[allow(unused)]
    pub fn index_path(&self) -> PathBuf {
        self.base_path.join("index.toml")
    }
    #[allow(unused)]
    pub fn mirrors_path(&self) -> PathBuf {
        self.base_path.join("mirrors.toml")
    }
    #[allow(unused)]
    pub fn download_cache_path(&self) -> PathBuf {
        self.download_cache.clone()
    }
}

// === Usage ===
impl ZvNetwork {
    /// Download a given zig_tarball to the download cache, returns the path to the downloaded file after shasum verification
    /// alongside minisign file and other relevant details. See [DownloadResult].
    /// This function implements a comprehensive download system with retry logic, mirror failover,
    /// integrity verification, and detailed error handling and logging.
    /// This requires `ensure_mirror_manager()` to have been called first to load mirrors.
    pub(super) async fn download_version(
        &mut self,
        semver_version: &semver::Version,
        zig_tarball: &str,
        download_artifact: Option<&ArtifactInfo>,
    ) -> Result<ZigDownload, ZvError> {
        use crate::app::MAX_RETRIES;
        const TARGET: &str = "zv::network::download_version";

        if let Some(artifact) = download_artifact {
            tracing::debug!(target: TARGET,
                "Starting download: {zig_tarball} (version: {semver_version}, size: {size} bytes, checksum: {shasum})",
                shasum = artifact.shasum, size = artifact.size);
        } else {
            tracing::debug!(target: TARGET,
                "Starting download: {zig_tarball} (version: {semver_version}) - no artifact info available");
        }

        let (shasum, size) = if let Some(artifact) = download_artifact {
            (Some(&artifact.shasum), Some(artifact.size))
        } else {
            (None, None)
        };

        // Ensure mirror manager is loaded first. This is already done in app.install_release() so it's an error to not have it loaded
        // Also, we make sure of this by limiting visibility of this function to app module only
        if self.mirror_manager.is_none() {
            Err(ZvError::NetworkError(NetErr::EmptyMirrors))?;
        }
        let mirror_manager = self.mirror_manager.as_mut().unwrap();
        let temp_dir = self.download_cache.join("tmp");
        if !temp_dir.exists() {
            tracing::debug!(target: TARGET, "Creating temporary download directory: {}", temp_dir.display());
            if let Err(e) = tokio::fs::create_dir_all(&temp_dir).await {
                tracing::error!(target: TARGET, "Failed to create temporary download directory {}: {}", temp_dir.display(), e);
                crate::tools::error("Failed to create temporary download directory");
                return Err(ZvError::Io(e));
            }
        }

        let temp_tarball_path = temp_dir.join(format!("{}.tmp", zig_tarball));
        let temp_minisig_path = temp_dir.join(format!("{}.minisig.tmp", zig_tarball));
        let final_tarball_path = self.download_cache.join(zig_tarball);
        let final_minisig_path = self.download_cache.join(format!("{}.minisig", zig_tarball));
        let progress_handle = ProgressHandle::spawn();
        let max_retries = *MAX_RETRIES;
        let mut last_error = None;

        // Clean up any existing temporary files from previous failed attempts
        remove_files(&[temp_tarball_path.as_path(), temp_minisig_path.as_path()]).await;
        let _ = mirror_manager
            .sort_by_rank()
            .await
            .map_err(ZvError::NetworkError)?;

        for attempt in 1..=max_retries {
            if let Some(s) = size {
                tracing::debug!(target: TARGET, "Download attempt {attempt}/{max_retries} for {zig_tarball} (expected size: {:.1} MB)", s as f64 / 1_048_576.0);
            } else {
                tracing::debug!(target: TARGET, "Download attempt {attempt}/{max_retries} for {zig_tarball} (expected size: unknown)");
            }
            // Select mirror based on attempt number
            let selected_mirror = {
                // For subsequent attempts, get ranked mirrors and select the best available
                match mirror_manager.get_random_mirror().await {
                    Ok(ranked_mirror) => ranked_mirror,
                    Err(net_err) => {
                        tracing::error!(target: TARGET, "Failed to get ranked mirror for attempt {attempt}: {net_err}");
                        last_error = Some(ZvError::NetworkError(net_err));
                        continue;
                    }
                }
            }; // end select_mirror

            tracing::trace!(target: TARGET, "Using mirror: {} (rank: {}) for attempt {}/{}", 
                         selected_mirror.base_url, selected_mirror.rank, attempt, max_retries);

            // Attempt download with this mirror
            match selected_mirror
                .download(
                    &self.client,
                    semver_version,
                    zig_tarball,
                    &temp_tarball_path,
                    &temp_minisig_path,
                    shasum.map(|s| s.as_str()),
                    size,
                    &progress_handle,
                )
                .await
            {
                Ok(()) => {
                    // Download successful, move files to final location
                    match move_to_final_location(&temp_tarball_path, &final_tarball_path).await {
                        Ok(()) => {
                            tracing::trace!(target: TARGET, "Successfully moved tarball to final location: {}", final_tarball_path.display());
                        }
                        Err(e) => {
                            tracing::error!(target: TARGET, "Failed to move tarball from {} to {}: {}", 
                                          temp_tarball_path.display(), final_tarball_path.display(), e);
                            let _ = progress_handle
                                .finish_with_error("Failed to move downloaded files")
                                .await;
                            return Err(ZvError::Io(e));
                        }
                    }

                    match move_to_final_location(&temp_minisig_path, &final_minisig_path).await {
                        Ok(()) => {
                            tracing::trace!(target: TARGET, "Successfully moved minisig to final location: {}", final_minisig_path.display());
                        }
                        Err(e) => {
                            tracing::error!(target: TARGET, "Failed to move minisig from {} to {}: {}", 
                                          temp_minisig_path.display(), final_minisig_path.display(), e);
                            let _ = progress_handle
                                .finish_with_error("Failed to move signature file")
                                .await;
                            return Err(ZvError::Io(e));
                        }
                    }

                    // Promote the successful mirror and save rankings
                    let old_rank = selected_mirror.rank;
                    selected_mirror.promote();
                    tracing::trace!(target: TARGET, "Promoting successful mirror {} from rank {} to {}", 
                                 selected_mirror.base_url, old_rank, selected_mirror.rank);

                    match progress_handle
                        .finish("Download completed successfully")
                        .await
                    {
                        Ok(()) => {
                            tracing::trace!(target: TARGET, "Progress handle finished successfully");
                        }
                        Err(e) => {
                            tracing::trace!(target: TARGET, "Failed to finish progress handle: {} - This is non-critical", e);
                        }
                    }

                    if let Some(s) = size {
                        tracing::debug!(target: TARGET, "Successfully downloaded {} ({:.1} MB) using mirror {} after {} attempt(s)", 
                                     zig_tarball, s as f64 / 1_048_576.0, selected_mirror.base_url, attempt);
                    } else {
                        tracing::debug!(target: TARGET, "Successfully downloaded {} using mirror {} after {} attempt(s)", 
                                     zig_tarball, selected_mirror.base_url, attempt);
                    }

                    let download_result = ZigDownload {
                        tarball_path: final_tarball_path,
                        minisig_path: final_minisig_path,
                        mirror_used: selected_mirror.base_url.to_string(),
                    };

                    // Update mirror ranking on disk
                    if let Err(e) = mirror_manager.save_index_to_disk().await {
                        tracing::debug!(target: TARGET, "Failed to update mirror ranking after successful download: {} - Rankings may not persist", e);
                    } else {
                        tracing::trace!(target: TARGET, "Successfully updated and persisted mirror rankings");
                    }

                    return Ok(download_result);
                }
                Err(err) => {
                    tracing::warn!(target: TARGET, "Download attempt {}/{} failed with mirror {} (rank: {}): {}", 
                                 attempt, max_retries, selected_mirror.base_url, selected_mirror.rank, err);

                    // Demote the failed mirror and save rankings
                    let old_rank = selected_mirror.rank;
                    selected_mirror.demote();
                    tracing::debug!(target: TARGET, "Demoting failed mirror {} from rank {} to rank {}", 
                                 selected_mirror.base_url, old_rank, selected_mirror.rank);

                    // Update mirror ranking
                    if let Err(e) = mirror_manager.save_index_to_disk().await {
                        tracing::warn!(target: TARGET, "Failed to update mirror ranking after failure: {} - Mirror performance tracking may be affected", e);
                    } else {
                        tracing::debug!(target: TARGET, "Successfully updated mirror rankings after failure");
                    }

                    // Clean up temporary files after download
                    remove_files(&[temp_tarball_path.as_path(), temp_minisig_path.as_path()]).await;

                    last_error = Some(err.into());

                    // Check if this is the last attempt
                    if attempt == max_retries {
                        tracing::error!(target: TARGET, "Final attempt {}/{} failed - no more retries available", attempt, max_retries);
                        break;
                    }

                    // Log retry message with context
                    tracing::debug!(target: TARGET, "Will retry download with different mirror (next attempt: {}/{}) after mirror failure", 
                                 attempt + 1, max_retries);
                }
            }
        }

        // All attempts failed - provide comprehensive error reporting
        tracing::error!(target: TARGET, "All {} download attempts failed for {} - exhausted all retry options", max_retries, zig_tarball);

        // Ensure progress handle is properly finished with error context
        match progress_handle
            .finish_with_error(&format!("Download failed after {} attempts", max_retries))
            .await
        {
            Ok(()) => {
                tracing::debug!(target: TARGET, "Progress handle finished with error message");
            }
            Err(e) => {
                tracing::warn!(target: TARGET, "Failed to finish progress handle with error: {} - This is non-critical but affects user experience", e);
            }
        }

        // Final cleanup attempt for any remaining temporary files
        remove_files(&[temp_tarball_path.as_path(), temp_minisig_path.as_path()]).await;

        let final_error = last_error.unwrap_or_else(|| {
            tracing::error!(target: TARGET, "No specific error recorded - this indicates a critical issue with mirror availability");
            ZvError::NetworkError(crate::NetErr::EmptyMirrors)
        });
        Err(final_error)
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
            Ok(index) => match index.contains_version(version).cloned() {
                Some(release) => Ok(release),
                None => {
                    // Try updating zig index first. Maybe the semver is newer than our index contents and TTL hasn't refreshed index
                    match self
                        .index_manager
                        .ensure_loaded(CacheStrategy::AlwaysRefresh)
                        .await
                    {
                        Ok(updated_index) => updated_index
                            .contains_version(version)
                            .cloned()
                            .ok_or_else(|| {
                                ZvError::ZigNotFound(eyre!(
                                    "Version {} not found in Zig download index after refresh",
                                    version
                                ))
                            }),

                        Err(network_err) => {
                            tracing::error!(
                                target: "zv::network::validate_semver",
                                "Failed to refresh index from network: {network_err}. Cannot validate version"
                            );
                            Err(ZvError::ZigNotFound(
                                eyre!("Version {} not found in Zig download index", version)
                                    .wrap_err(network_err),
                            ))
                        }
                    }
                }
            },
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
                    && let Some(cached_master) =
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
                            Ok(index) => {
                                index.get_latest_stable_release().cloned().ok_or_else(|| {
                                    ZvError::ZigVersionResolveError(eyre!(
                                        "No stable version found in cached index"
                                    ))
                                })
                            }
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

    /// Direct download function for --force-ziglang mode
    /// Downloads tarball and minisig directly from ziglang.org, verifies checksum and minisign signature
    pub async fn direct_download(
        &self,
        tarball_url: &str,
        minisig_url: &str,
        zig_tarball: &str,
        expected_shasum: Option<&str>,
        expected_size: Option<u64>,
    ) -> Result<ZigDownload, ZvError> {
        const TARGET: &str = "zv::network::direct_download";

        tracing::debug!(target: TARGET, "Starting direct download from ziglang.org");
        tracing::debug!(target: TARGET, "Tarball URL: {}", tarball_url);
        tracing::debug!(target: TARGET, "Minisig URL: {}", minisig_url);
        if let Some(size) = expected_size {
            tracing::debug!(target: TARGET, "Expected size: {} bytes ({:.1} MB)", size, size as f64 / 1_048_576.0);
        } else {
            tracing::debug!(target: TARGET, "Expected size: unknown");
        }
        if let Some(shasum) = expected_shasum {
            tracing::debug!(target: TARGET, "Expected checksum: {}", shasum);
        } else {
            tracing::debug!(target: TARGET, "Expected checksum: unknown");
        }

        // Ensure download cache directory exists
        if !self.download_cache.exists() {
            tokio::fs::create_dir_all(&self.download_cache)
                .await
                .map_err(ZvError::Io)
                .wrap_err("Failed to create download cache directory")?;
        }

        let final_tarball_path = self.download_cache.join(zig_tarball);
        let final_minisig_path = self.download_cache.join(format!("{}.minisig", zig_tarball));

        let progress_handle = ProgressHandle::spawn();

        // Phase 1: Download tarball directly from ziglang.org
        tracing::debug!(target: TARGET, "Downloading tarball directly from {}", tarball_url);
        let progress_msg = format!("Downloading {} from ziglang.org", zig_tarball);
        if let Err(e) = progress_handle.start(&progress_msg).await {
            tracing::debug!(target: TARGET, "Failed to start progress reporting: {} - continuing without progress updates", e);
        }

        stream_download_file(
            &self.client,
            tarball_url,
            &final_tarball_path,
            expected_size.unwrap_or(0),
            &progress_handle,
        )
        .await
        .map_err(ZvError::NetworkError)?;

        // Phase 2: Verify checksum (if available)
        if let Some(shasum) = expected_shasum {
            tracing::debug!(target: TARGET, "Verifying tarball checksum");
            verify_checksum(&final_tarball_path, shasum).await?;
        } else {
            tracing::debug!(target: TARGET, "Skipping checksum verification - no expected checksum provided");
        }

        // Phase 3: Download minisig file directly from ziglang.org
        tracing::debug!(target: TARGET, "Downloading signature file directly from {}", minisig_url);
        if let Err(e) = progress_handle
            .update("Downloading signature file...")
            .await
        {
            tracing::warn!(target: TARGET, "Failed to update progress for minisig download: {} - continuing", e);
        }

        stream_download_file(
            &self.client,
            minisig_url,
            &final_minisig_path,
            0, // minisig files are small, size unknown
            &progress_handle,
        )
        .await
        .map_err(ZvError::NetworkError)?;

        // Phase 4: Verify minisign signature
        tracing::debug!(target: TARGET, "Verifying minisign signature");
        if let Err(e) = progress_handle.update("Verifying signature...").await {
            tracing::warn!(target: TARGET, "Failed to update progress for signature verification: {} - continuing", e);
        }

        crate::app::minisign::verify_minisign_signature(&final_tarball_path, &final_minisig_path)?;

        // Finish progress reporting
        if let Err(e) = progress_handle
            .finish("Download and verification completed")
            .await
        {
            tracing::debug!(target: TARGET, "Failed to finish progress handle: {} - This is non-critical", e);
        }

        if let Some(size) = expected_size {
            tracing::debug!(target: TARGET, "Successfully downloaded and verified {} ({:.1} MB) from ziglang.org", 
                         zig_tarball, size as f64 / 1_048_576.0);
        } else {
            tracing::debug!(target: TARGET, "Successfully downloaded {} from ziglang.org", zig_tarball);
        }

        Ok(ZigDownload {
            tarball_path: final_tarball_path,
            minisig_path: final_minisig_path,
            mirror_used: tarball_url.to_string(),
        })
    }
}

pub(crate) fn create_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(zv_agent())
        .pool_max_idle_per_host(0) // Don't keep idle connections
        .connect_timeout(Duration::from_secs(10))
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
        .timeout(Duration::from_secs(2))
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
    let master_start = json_text.find(r#""master":"#).ok_or_else(|| {
        eyre!(
            "Could not find master key in partial JSON (length: {})",
            json_text.len()
        )
    })?;

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
    let network_release: NetworkZigRelease = serde_json::from_str(master_json).map_err(|e| {
        eyre!(
            "Failed to parse extracted master JSON (length: {}): {e}",
            master_json.len()
        )
    })?;

    // Convert to ZigRelease
    let resolved_version = if let Some(version_str) = &network_release.version {
        match semver::Version::parse(version_str) {
            Ok(version) => ResolvedZigVersion::Master(version),
            Err(_) => {
                tracing::warn!("Failed to parse master version: {}", version_str);
                return Err(eyre!(
                    "Master version without valid semver: {}",
                    version_str
                ));
            }
        }
    } else {
        tracing::warn!("Master release found without version information");
        return Err(eyre!("Master release missing version information"));
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

    let master_release = ZigRelease::new(resolved_version, network_release.date, runtime_artifacts);

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
