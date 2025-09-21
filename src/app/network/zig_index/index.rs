//! Zig download index representation and management

use super::models::{ArtifactInfo, CacheZigIndex, NetworkZigIndex, ZigIndex, ZigRelease};
use crate::{
    CfgErr, NetErr, ZigVersion, ZvError,
    app::{
        INDEX_TTL_DAYS, NETWORK_TIMEOUT_SECS,
        constants::ZIG_DOWNLOAD_INDEX_JSON,
        network::{CacheStrategy, TARGET},
    },
    types::ResolvedZigVersion,
};
use chrono::{DateTime, Utc};
use reqwest::Client;
use std::{path::PathBuf, str::FromStr};

// Backward compatibility wrapper for ZigRelease
impl ZigRelease {
    /// Get version as string
    pub fn version_string(&self) -> String {
        // Extract the actual semver version from ResolvedZigVersion
        match self.resolved_version() {
            ResolvedZigVersion::Semver(v) => v.to_string(),
            ResolvedZigVersion::Master(v) => v.to_string(),
        }
    }

    /// Fast target-support check (backward compatibility)
    pub fn has_target(&self, triple: &str) -> bool {
        use crate::types::TargetTriple;
        if let Some(target_triple) = TargetTriple::from_key(triple) {
            self.artifacts().contains_key(&target_triple)
        } else {
            false
        }
    }

    /// Borrow the artifact for a target (backward compatibility)
    pub fn target_artifact(&self, triple: &str) -> Option<&ArtifactInfo> {
        use crate::types::TargetTriple;
        tracing::debug!(target: TARGET, "Length of targets: {}", self.artifacts().len());
        for k in self.artifacts().keys() {
            tracing::debug!(target: TARGET, "Available target: {}", k.to_key());
        }

        if let Some(target_triple) = TargetTriple::from_key(triple) {
            self.artifacts().get(&target_triple)
        } else {
            None
        }
    }

    /// ziglang tarball URL for a target (backward compatibility)
    pub fn ziglang_org_tarball_url(&self, triple: &str) -> Option<&str> {
        self.target_artifact(triple)
            .map(|a| a.ziglang_org_tarball.as_str())
    }

    /// Convenience: SHA-256 for a target (backward compatibility)
    pub fn shasum(&self, triple: &str) -> Option<&str> {
        self.target_artifact(triple).map(|a| a.shasum.as_str())
    }

    /// Convenience: size in bytes for a target (backward compatibility)
    pub fn size(&self, triple: &str) -> Option<u64> {
        self.target_artifact(triple).map(|a| a.size)
    }

    /// Iterator over all supported triples for this release (backward compatibility)
    pub fn targets(&self) -> impl Iterator<Item = String> + '_ {
        self.artifacts().keys().map(|t| t.to_key())
    }
}

#[derive(Debug, Clone)]
/// In memory index manager for zig download index
pub struct IndexManager {
    client: Client,
    index_path: PathBuf,
    index: Option<ZigIndex>,
}

impl IndexManager {
    /// Creates a new `IndexManager` instance with the specified index path and HTTP client.
    ///
    /// # Arguments
    ///
    /// * `index_path` - The file path where the index will be cached on disk.
    /// * `client` - A reqwest client for making network requests.
    pub fn new(index_path: PathBuf, client: Client) -> Self {
        Self {
            index_path,
            index: None,
            client,
        }
    }

    /// Returns a reference to the loaded `ZigIndex` if available.
    ///
    /// Note: Consider using `ensure_loaded` instead to guarantee the index is available.
    pub fn get_index(&self) -> Option<&ZigIndex> {
        self.index.as_ref()
    }

    /// Ensures the index is loaded based on the provided cache strategy.
    ///
    /// This method handles loading the index from disk or fetching it from the network
    /// depending on the `CacheStrategy`. It updates the internal state with the loaded or fetched index.
    ///
    /// # Arguments
    ///
    /// * `cache_strategy` - The strategy to use for caching and loading the index.
    ///
    /// # Returns
    ///
    /// Returns `Ok(&ZigIndex)` on success, or a `ZvError` if loading or fetching fails.
    pub async fn ensure_loaded(
        &mut self,
        cache_strategy: CacheStrategy,
    ) -> Result<&ZigIndex, ZvError> {
        match cache_strategy {
            CacheStrategy::AlwaysRefresh => {
                // Always fetch fresh data from network
                tracing::debug!(target: TARGET, "Refreshing index - fetching from network");
                self.refresh_from_network().await?;
            }
            CacheStrategy::PreferCache => {
                // Use cached data if available, only fetch if no cache exists
                if self.index_path.is_file() {
                    let data =
                        tokio::fs::read_to_string(&self.index_path)
                            .await
                            .map_err(|io_err| {
                                ZvError::ZvConfigError(CfgErr::NotFound(io_err.into()))
                            })?;

                    let cache_index: CacheZigIndex = toml::from_str(&data)
                        .map_err(|e| ZvError::ZvConfigError(CfgErr::ParseFail(e.into())))?;

                    let runtime_index: ZigIndex = cache_index.into();
                    self.index = Some(runtime_index);
                    tracing::debug!(target: TARGET, "Using cached index");
                } else {
                    tracing::debug!(target: TARGET, "No cache found - fetching from network");
                    self.refresh_from_network().await?;
                }
            }
            CacheStrategy::RespectTtl => {
                // Respect TTL - use cache if not expired, otherwise refresh
                if self.index_path.is_file() {
                    let data =
                        tokio::fs::read_to_string(&self.index_path)
                            .await
                            .map_err(|io_err| {
                                ZvError::ZvConfigError(CfgErr::NotFound(io_err.into()))
                            })?;

                    let cache_index = toml::from_str::<CacheZigIndex>(&data).map_err(|e| {
                        tracing::error!(target: TARGET, "Parse error on cached zig index: {e}");
                        e
                    });

                    if cache_index.is_err() {
                        tracing::debug!(target: TARGET, "zig index - refreshing from network");
                        self.refresh_from_network().await?;
                        return Ok(self
                            .index
                            .as_ref()
                            .expect("Index should be loaded after refresh"));
                    }
                    let cache_index = cache_index.unwrap();
                    let runtime_index: ZigIndex = cache_index.into();
                    if runtime_index.is_expired() {
                        tracing::debug!(target: TARGET, "Cache expired - refreshing from network");
                        self.refresh_from_network().await?;
                    } else {
                        tracing::debug!(target: TARGET, "Using valid cached index");
                        self.index = Some(runtime_index);
                    }
                } else {
                    tracing::debug!(target: TARGET, "No cache found - fetching from network");
                    self.refresh_from_network().await?;
                }
            }
            CacheStrategy::OnlyCache => {
                // Use cached data if available, only fetch if no cache exists
                if self.index_path.is_file() {
                    let data =
                        tokio::fs::read_to_string(&self.index_path)
                            .await
                            .map_err(|io_err| {
                                ZvError::ZvConfigError(CfgErr::NotFound(io_err.into()))
                            })?;

                    let cache_index: CacheZigIndex = toml::from_str(&data)
                        .map_err(|e| ZvError::ZvConfigError(CfgErr::ParseFail(e.into())))?;

                    let runtime_index: ZigIndex = cache_index.into();
                    self.index = Some(runtime_index);
                    tracing::debug!(target: TARGET, "Using cached index");
                } else {
                    tracing::debug!(target: TARGET, "No cache found - OnlyCache strategy... returning");
                    return Err(ZvError::CacheNotFound(
                        self.index_path.to_string_lossy().to_string(),
                    ));
                }
            }
        }

        Ok(self
            .index
            .as_ref()
            .expect("Index should be loaded after ensure_loaded"))
    }

    /// Saves the current in-memory index to disk as a TOML file.
    ///
    /// If no index is loaded, this method does nothing.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `CfgErr` if serialization or writing fails.
    pub async fn save_to_disk(&self) -> Result<(), CfgErr> {
        if let Some(ref runtime_index) = self.index {
            // Convert runtime index to cache index for TOML serialization
            let cache_index = CacheZigIndex::from(runtime_index);
            let toml_str =
                toml::to_string_pretty(&cache_index).map_err(|e| CfgErr::ParseFail(e.into()))?;
            tokio::fs::write(&self.index_path, toml_str)
                .await
                .map_err(|io_err| {
                    CfgErr::WriteFail(io_err.into(), self.index_path.to_string_lossy().to_string())
                })?;
        }
        Ok(())
    }

    /// Fetches the latest index from the network, updates the internal state, and attempts to save it to disk.
    ///
    /// The index is fetched from `ZIG_DOWNLOAD_INDEX_JSON`, parsed as JSON, and the `last_synced` timestamp is updated.
    /// If saving to disk fails, it is logged as a warning but does not fail the operation.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `ZvError` if the network request or parsing fails.
    pub async fn refresh_from_network(&mut self) -> Result<(), ZvError> {
        let response = self
            .client
            .get(ZIG_DOWNLOAD_INDEX_JSON)
            .timeout(std::time::Duration::from_secs(*NETWORK_TIMEOUT_SECS))
            .send()
            .await
            .map_err(NetErr::Reqwest)
            .map_err(ZvError::NetworkError)?;
        if !response.status().is_success() {
            return Err(ZvError::NetworkError(NetErr::HTTP(response.status())));
        }

        let text = response
            .text()
            .await
            .map_err(NetErr::Reqwest)
            .map_err(ZvError::NetworkError)?;

        // Deserialize as NetworkZigIndex and convert to ZigIndex
        let network_index = serde_json::from_str::<NetworkZigIndex>(&text)
            .map_err(NetErr::JsonParse)
            .map_err(ZvError::NetworkError)?;

        let runtime_index: ZigIndex = network_index.into();

        self.index = Some(runtime_index);
        let _ = self.save_to_disk().await.map_err(|e| {
            // Non-fatal error, log and continue
            tracing::warn!(target: TARGET, "Failed to save refreshed index to disk: {}", e);
        });
        Ok(())
    }
}
