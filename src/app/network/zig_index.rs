//! Zig download index representation and management

use crate::{
    CfgErr, NetErr, ZigVersion, ZvError,
    app::{
        constants::ZIG_DOWNLOAD_INDEX_JSON,
        network::{CacheStrategy, INDEX_TTL_DAYS, TARGET},
    },
};
use chrono::{DateTime, Utc};
use reqwest::Client;
use reqwest_middleware::ClientWithMiddleware;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

/// Represents a download artifact with tarball URL, SHA sum, and size
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadArtifact {
    #[serde(rename = "tarball")]
    pub ziglang_org_tarball: String,
    pub shasum: String,
    #[serde(deserialize_with = "deserialize_str_to_u64")]
    pub size: u64,
}

/// Custom deserializer to convert string to u64 for size field
fn deserialize_str_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    s.parse::<u64>().map_err(serde::de::Error::custom)
}

/// Represents a Zig release version
#[derive(Debug, Clone)]
pub struct ZigRelease {
    /// Semver version string (optional for regular releases, required for master)
    pub version: String,
    /// Publish date
    pub date: String,
    /// Platform-specific artifacts
    pub targets: BTreeMap<String, DownloadArtifact>,
}

impl Serialize for ZigRelease {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        // Include version field only for master (dev versions)
        let include_version = !self.version.is_empty()
            && (self.version.contains("dev") || self.version.contains("+"));
        let map_size = if include_version { 2 } else { 1 } + self.targets.len();
        let mut map = serializer.serialize_map(Some(map_size))?;

        if include_version {
            map.serialize_entry("version", &self.version)?;
        }
        map.serialize_entry("date", &self.date)?;

        // Serialize targets
        for (key, artifact) in &self.targets {
            map.serialize_entry(key, artifact)?;
        }

        map.end()
    }
}

impl<'de> Deserialize<'de> for ZigRelease {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};

        struct ZigReleaseVisitor;

        impl<'de> Visitor<'de> for ZigReleaseVisitor {
            type Value = ZigRelease;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a ZigRelease object")
            }

            fn visit_map<V>(self, mut map: V) -> Result<ZigRelease, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut version = None;
                let mut date = None;
                let mut targets = BTreeMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "date" => {
                            date = Some(map.next_value()?);
                        }
                        "version" => {
                            // Capture version field if present (for master)
                            version = Some(map.next_value()?);
                        }
                        // Skip documentation, bootstrap, source, and other non-platform fields
                        "docs" | "stdDocs" | "langRef" | "notes" | "bootstrap" | "src" => {
                            let _: serde_json::Value = map.next_value()?;
                        }
                        // Everything else should be a platform target
                        _ => {
                            // Try to deserialize as DownloadArtifact, skip if it fails
                            match map.next_value::<DownloadArtifact>() {
                                Ok(artifact) => {
                                    targets.insert(key, artifact);
                                }
                                Err(_) => {
                                    // Skip fields that don't deserialize as DownloadArtifact
                                    // This handles cases where the value is a string or other type
                                }
                            }
                        }
                    }
                }

                let date = date.ok_or_else(|| serde::de::Error::missing_field("date"))?;

                Ok(ZigRelease {
                    version: version.unwrap_or_default(), // Will be overridden by ZigIndex deserializer if empty
                    date,
                    targets,
                })
            }
        }

        deserializer.deserialize_map(ZigReleaseVisitor)
    }
}

/// Main structure representing the Zig download index
#[derive(Debug, Clone)]
pub struct ZigIndex {
    /// All releases including master - BTreeMap for sorted keys
    pub releases: BTreeMap<String, ZigRelease>,

    /// Timestamp of when this index was last synced (not part of original JSON)
    pub last_synced: Option<DateTime<Utc>>,
}

impl Serialize for ZigIndex {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        // Calculate map size (releases + last_synced if present)
        let map_size = self.releases.len() + if self.last_synced.is_some() { 1 } else { 0 };
        let mut map = serializer.serialize_map(Some(map_size))?;

        // Serialize all releases
        for (key, release) in &self.releases {
            map.serialize_entry(key, release)?;
        }

        // Serialize last_synced if present (for local zv cache)
        if let Some(ref last_synced) = self.last_synced {
            map.serialize_entry("last_synced", last_synced)?;
        }

        map.end()
    }
}

impl<'de> Deserialize<'de> for ZigIndex {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};

        struct ZigIndexVisitor;

        impl<'de> Visitor<'de> for ZigIndexVisitor {
            type Value = ZigIndex;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str(
                    "a ZigIndex object (see https://ziglang.org/download/index.json for the expected format)"
                )
            }

            fn visit_map<V>(self, mut map: V) -> Result<ZigIndex, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut releases = BTreeMap::new();
                let mut last_synced = None;

                while let Some(key) = map.next_key::<String>()? {
                    if key == "last_synced" {
                        last_synced = map.next_value()?;
                    } else {
                        // This is a release (including master)
                        let mut release: ZigRelease = map.next_value()?;

                        // Set version from key if not already set from JSON
                        if release.version.is_empty() {
                            release.version = key.clone();
                        }

                        releases.insert(key, release);
                    }
                }

                Ok(ZigIndex {
                    releases,
                    last_synced,
                })
            }
        }

        deserializer.deserialize_map(ZigIndexVisitor)
    }
}

impl ZigIndex {
    /// Get the latest stable release version - Returns [ZigVersion::Semver]
    pub fn get_latest_stable(&self) -> Option<ZigVersion> {
        self.releases
            .keys()
            .filter(|k| *k != "master") // Filter out master
            .filter(|k| !k.contains("dev")) // Filter out dev versions
            .filter_map(|version_key| {
                semver::Version::parse(version_key)
                    .ok()
                    .filter(|v| v.pre.is_empty()) // Ensure it's not a prerelease
            })
            .max() // Get the maximum version using semver comparison
            .map(ZigVersion::Semver)
    }

    /// Get master version info - Returns [ZigVersion::Semver]
    pub fn get_master_version(&self) -> Option<ZigVersion> {
        self.releases
            .get("master")
            .and_then(|release| semver::Version::parse(&release.version).ok())
            .map(ZigVersion::Semver)
    }

    /// Get all available target platforms for a specific version
    pub fn get_targets_for_version(&self, version: &str) -> Vec<&str> {
        self.releases
            .get(version)
            .map(|release| release.targets.keys().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    /// Cache expired?
    pub fn is_expired(&self) -> bool {
        if let Some(last_synced) = self.last_synced {
            let age = Utc::now() - last_synced;
            age.num_days() >= *INDEX_TTL_DAYS
        } else {
            true // If never synced, consider it expired
        }
    }

    /// Update the last_synced timestamp (call after successful HTTP fetch)
    pub fn update_sync_time(&mut self) {
        self.last_synced = Some(Utc::now());
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
    /// Call [`Self::ensure_loaded`] before calling this to guarantee the index is loaded and safe to unwrap.
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
    /// Returns `Ok(())` on success, or a `ZvError` if loading or fetching fails.
    pub async fn ensure_loaded(&mut self, cache_strategy: CacheStrategy) -> Result<(), ZvError> {
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

                    let index: ZigIndex = toml::from_str(&data)
                        .map_err(|e| ZvError::ZvConfigError(CfgErr::ParseFail(e.into())))?;

                    self.index = Some(index);
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

                    let index = toml::from_str::<ZigIndex>(&data).map_err(|e| {
                        tracing::error!(target: TARGET, "Parse error on cached zig index: {e}");
                        e
                    });

                    if index.is_err() {
                        tracing::debug!(target: TARGET, "zig index - refreshing from network");
                        self.refresh_from_network().await?;
                        return Ok(());
                    }
                    let index = index.unwrap();
                    if index.is_expired() {
                        tracing::debug!(target: TARGET, "Cache expired - refreshing from network");
                        self.refresh_from_network().await?;
                    } else {
                        tracing::debug!(target: TARGET, "Using valid cached index");
                        self.index = Some(index);
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

                    let index: ZigIndex = toml::from_str(&data)
                        .map_err(|e| ZvError::ZvConfigError(CfgErr::ParseFail(e.into())))?;

                    self.index = Some(index);
                    tracing::debug!(target: TARGET, "Using cached index");
                } else {
                    tracing::debug!(target: TARGET, "No cache found - OnlyCache strategy... returning");
                    return Err(ZvError::CacheNotFound(
                        self.index_path.to_string_lossy().to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Saves the current in-memory index to disk as a TOML file.
    ///
    /// If no index is loaded, this method does nothing.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` on success, or a `CfgErr` if serialization or writing fails.
    pub async fn save_to_disk(&self) -> Result<(), CfgErr> {
        if let Some(ref index) = self.index {
            let toml_str =
                toml::to_string_pretty(index).map_err(|e| CfgErr::ParseFail(e.into()))?;
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
            .timeout(std::time::Duration::from_secs(*super::NETWORK_TIMEOUT_SECS))
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

        let mut index = serde_json::from_str::<ZigIndex>(&text)
            .map_err(NetErr::JsonParse)
            .map_err(ZvError::NetworkError)?;

        // Update last_synced timestamp
        index.update_sync_time();

        self.index = Some(index);
        let _ = self.save_to_disk().await.map_err(|e| {
            // Non-fatal error, log and continue
            tracing::warn!(target: TARGET, "Failed to save refreshed index to disk: {}", e);
        });
        Ok(())
    }
}
