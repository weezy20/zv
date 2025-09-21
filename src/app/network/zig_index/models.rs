//! These models represent the runtime layer of the three-layer architecture:
//! 1. Network Layer (NetworkZigIndex, NetworkZigRelease, NetworkArtifact) - for JSON deserialization
//! 2. Runtime Layer (ZigIndex, ZigRelease, ArtifactInfo) - for in-memory operations
//! 3. Cache Layer (CacheZigIndex, CacheZigRelease, CacheArtifact) - for TOML serialization

use crate::app::INDEX_TTL_DAYS;
use crate::app::utils::{host_target, zig_tarball};
use crate::types::{ResolvedZigVersion, TargetTriple, ZigVersion};
use chrono::{DateTime, Utc};
use serde::{
    Deserialize, Deserializer, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::str::FromStr;

/// Raw JSON representation from ziglang.org
#[derive(Debug, Deserialize)]
pub struct NetworkZigIndex {
    #[serde(flatten)]
    pub releases: HashMap<String, NetworkZigRelease>,
}

/// Represents a Zig release from the network JSON
#[derive(Debug)]
pub struct NetworkZigRelease {
    pub date: String,
    pub version: Option<String>, // Only present for master
    pub targets: HashMap<String, NetworkArtifact>,
}

/// Represents a download artifact from the network JSON
#[derive(Debug, Deserialize)]
pub struct NetworkArtifact {
    #[serde(rename = "tarball")]
    pub ziglang_org_tarball: String,
    pub shasum: String,
    #[serde(deserialize_with = "deserialize_str_to_u64")]
    pub size: u64,
}

/// Custom deserializer to convert string to u64 for size field
fn deserialize_str_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    s.parse::<u64>().map_err(de::Error::custom)
}

impl<'de> Deserialize<'de> for NetworkZigRelease {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct NetworkZigReleaseVisitor;

        impl<'de> Visitor<'de> for NetworkZigReleaseVisitor {
            type Value = NetworkZigRelease;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a NetworkZigRelease object")
            }

            fn visit_map<V>(self, mut map: V) -> Result<NetworkZigRelease, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut date = None;
                let mut version = None;
                let mut targets = HashMap::new();

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
                            // Try to deserialize as NetworkArtifact, skip if it fails
                            match map.next_value::<NetworkArtifact>() {
                                Ok(artifact) => {
                                    targets.insert(key, artifact);
                                }
                                Err(_) => {
                                    // Skip fields that don't deserialize as NetworkArtifact
                                    // This handles cases where the value is a string or other type
                                }
                            }
                        }
                    }
                }

                let date = date.ok_or_else(|| de::Error::missing_field("date"))?;

                Ok(NetworkZigRelease {
                    date,
                    version,
                    targets,
                })
            }
        }

        deserializer.deserialize_map(NetworkZigReleaseVisitor)
    }
}

// ============================================================================
// Cache Layer Models for TOML Serialization
// ============================================================================

/// Simplified TOML representation of the Zig index for local caching
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheZigIndex {
    /// List of releases using array structure for clean TOML output
    pub releases: Vec<CacheZigRelease>,
    /// Timestamp of when this index was last synced
    pub last_synced: Option<DateTime<Utc>>,
}

/// Simplified TOML representation of a Zig release
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheZigRelease {
    /// Version string (e.g., "0.11.0", "master")
    pub version: String,
    /// Release date
    pub date: String,
    /// List of artifacts using array structure for clean TOML output
    pub artifacts: Vec<CacheArtifact>,
}

/// Simplified TOML representation of a download artifact
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheArtifact {
    /// Target triple in "arch-os" format
    pub target: String,
    /// Tarball download URL
    pub tarball_url: String,
    /// SHA-256 checksum
    pub shasum: String,
    /// Size in bytes
    pub size: u64,
}

/// Clean artifact data optimized for runtime operations
#[derive(Debug, Clone)]
pub struct ArtifactInfo {
    /// Tarball download URL from ziglang.org
    pub ziglang_org_tarball: String,
    /// SHA-256 checksum
    pub shasum: String,
    /// Size in bytes
    pub size: u64,
}

/// Runtime representation of a Zig release optimized for fast lookups
#[derive(Debug, Clone)]
pub struct ZigRelease {
    /// Version information
    version: ResolvedZigVersion,
    /// Release date
    date: String,
    /// Map of target triples to artifact information
    artifacts: HashMap<TargetTriple, ArtifactInfo>,
}

impl ZigRelease {
    /// Create a new ZigRelease
    pub fn new(
        version: ResolvedZigVersion,
        date: String,
        artifacts: HashMap<TargetTriple, ArtifactInfo>,
    ) -> Self {
        Self {
            version,
            date,
            artifacts,
        }
    }

    /// Get the version of this release
    pub fn resolved_version(&self) -> &ResolvedZigVersion {
        &self.version
    }

    /// Get the release date
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Get all available artifacts
    pub fn artifacts(&self) -> &HashMap<TargetTriple, ArtifactInfo> {
        &self.artifacts
    }

    /// Generate tarball URL for the current host system
    /// Returns None if the target is not supported or no artifact is available
    pub fn zig_tarball_for_current_host(&self) -> Option<String> {
        let host_target_str = host_target()?;
        let target_triple = TargetTriple::from_key(&host_target_str)?;
        self.zig_tarball_for_target(&target_triple)
    }

    /// Generate tarball URL for a specific target
    /// Returns None if the target is not supported or no artifact is available
    pub fn zig_tarball_for_target(&self, target: &TargetTriple) -> Option<String> {
        // Check if we have an artifact for this target
        if !self.artifacts.contains_key(target) {
            return None;
        }

        // Extract semver::Version from our ResolvedZigVersion
        let semver_version = match &self.version {
            ResolvedZigVersion::Semver(v) => v,
            ResolvedZigVersion::Master(v) => v,
        };

        // Generate tarball name for the specific target
        // Use the same logic as zig_tarball but with the provided target
        self.zig_tarball_for_target_and_version(&target.arch, &target.os, semver_version)
    }

    /// Helper function to generate tarball name for a specific arch, os, and version
    /// This follows the same logic as the existing zig_tarball utility but for arbitrary targets
    fn zig_tarball_for_target_and_version(
        &self,
        arch: &str,
        os: &str,
        semver_version: &semver::Version,
    ) -> Option<String> {
        // Determine the appropriate file extension based on the OS
        let ext = if os == "windows" { "zip" } else { "tar.xz" };

        // Handle the naming convention change in Zig 0.14.1
        if semver_version.le(&semver::Version::new(0, 14, 0)) {
            Some(format!("zig-{os}-{arch}-{semver_version}.{ext}"))
        } else {
            Some(format!("zig-{arch}-{os}-{semver_version}.{ext}"))
        }
    }
}

/// Runtime representation of the Zig index optimized for fast lookups
#[derive(Debug, Clone)]
pub struct ZigIndex {
    /// Map of versions to releases, sorted by version
    releases: BTreeMap<ResolvedZigVersion, ZigRelease>,
    /// Timestamp of when this index was last synced
    last_synced: Option<DateTime<Utc>>,
}

impl ZigIndex {
    /// Create a new empty ZigIndex
    pub fn new() -> Self {
        Self {
            releases: BTreeMap::new(),
            last_synced: None,
        }
    }

    /// Create a new ZigIndex with releases and sync timestamp
    pub fn with_releases(
        releases: BTreeMap<ResolvedZigVersion, ZigRelease>,
        last_synced: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            releases,
            last_synced,
        }
    }

    /// Get all releases
    pub fn releases(&self) -> &BTreeMap<ResolvedZigVersion, ZigRelease> {
        &self.releases
    }

    /// Get the last sync timestamp
    pub fn last_synced(&self) -> Option<DateTime<Utc>> {
        self.last_synced
    }

    /// Get artifact information for a specific version and target
    pub fn get_artifact(
        &self,
        version: &ResolvedZigVersion,
        target: &TargetTriple,
    ) -> Option<&ArtifactInfo> {
        self.releases.get(version)?.artifacts.get(target)
    }

    /// Get artifact information for a specific version and the current host target
    pub fn get_host_artifact(&self, version: &ResolvedZigVersion) -> Option<&ArtifactInfo> {
        let host_target_str = host_target()?;
        let target_triple = TargetTriple::from_key(&host_target_str)?;
        self.get_artifact(version, &target_triple)
    }

    /// Get all available targets for a specific version
    pub fn get_available_targets(&self, version: &ResolvedZigVersion) -> Vec<&TargetTriple> {
        match self.releases.get(version) {
            Some(release) => release.artifacts.keys().collect(),
            None => Vec::new(),
        }
    }

    /// Check if a version exists in the index
    pub fn has_version(&self, version: &ResolvedZigVersion) -> bool {
        self.releases.contains_key(version)
    }

    /// Get the latest stable version
    /// Returns the highest semantic version that is not a pre-release
    pub fn get_latest_stable(&self) -> Option<&ResolvedZigVersion> {
        self.releases
            .keys()
            .rev() // Start from highest versions
            .find(|version| {
                match version {
                    ResolvedZigVersion::Semver(v) => {
                        // Only consider stable releases (no pre-release or build metadata)
                        v.pre.is_empty() && v.build.is_empty()
                    }
                    _ => false, // Master variants are not considered stable
                }
            })
    }
}

// Backward compatibility wrapper for ZigIndex
impl ZigIndex {
    /// Check if a semver is in index (backward compatibility)
    pub fn contains_version(&self, version: &semver::Version) -> Option<&ZigRelease> {
        let resolved_version = ResolvedZigVersion::Semver(version.clone());
        self.releases().get(&resolved_version)
    }

    /// Get the latest stable release version (backward compatibility)
    pub fn get_latest_stable_release(&self) -> Option<&ZigRelease> {
        if let Some(latest_version) = self.get_latest_stable() {
            self.releases().get(latest_version)
        } else {
            None
        }
    }

    /// Get master version info (backward compatibility)
    pub fn get_master_version(&self) -> Option<&ZigRelease> {
        // Look for any master version in the index
        for (version, release) in self.releases() {
            if version.is_master() {
                return Some(release);
            }
        }

        None
    }

    /// Get all available target platforms for a specific version (backward compatibility)
    pub fn get_targets_for_version(&self, version: &str) -> Vec<String> {
        // Try to find the version in the index by string matching
        for (resolved_version, _) in self.releases() {
            let version_string = match resolved_version {
                ResolvedZigVersion::Semver(v) => v.to_string(),
                ResolvedZigVersion::Master(v) => v.to_string(),
            };

            if version_string == version || (version == "master" && resolved_version.is_master()) {
                return self
                    .get_available_targets(resolved_version)
                    .into_iter()
                    .map(|t| t.to_key())
                    .collect();
            }
        }
        Vec::new()
    }

    /// Cache expired? (backward compatibility)
    pub fn is_expired(&self) -> bool {
        if let Some(last_synced) = self.last_synced() {
            let age = Utc::now() - last_synced;
            age.num_days() >= *INDEX_TTL_DAYS
        } else {
            true // If never synced, consider it expired
        }
    }

    /// Update the last_synced timestamp (backward compatibility)
    pub fn update_sync_time(&mut self) {
        // This method can't be implemented on the immutable ZigIndex
        // The sync time is set during conversion from NetworkZigIndex
        tracing::warn!(
            "update_sync_time called on ZigIndex - this is a no-op. Sync time is set during network conversion."
        );
    }

    /// Access to releases as string keys (backward compatibility)
    pub fn releases_by_string(&self) -> std::collections::BTreeMap<String, &ZigRelease> {
        let mut result = std::collections::BTreeMap::new();
        for (version, release) in self.releases() {
            let key = match version {
                ResolvedZigVersion::Semver(v) => v.to_string(),
                ResolvedZigVersion::Master(v) => v.to_string(),
            };
            result.insert(key, release);
        }
        result
    }
}

impl Default for ZigIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Conversion Methods Between Layers
// ============================================================================

impl From<NetworkZigIndex> for ZigIndex {
    fn from(network_index: NetworkZigIndex) -> Self {
        let mut releases = BTreeMap::new();

        for (version_key, network_release) in network_index.releases {
            // Parse the version key to determine the ResolvedZigVersion
            let resolved_version = if version_key == "master" {
                // For master, use the version field if available
                if let Some(version_str) = &network_release.version {
                    match semver::Version::parse(version_str) {
                        Ok(version) => ResolvedZigVersion::Master(version),
                        Err(_) => {
                            tracing::warn!("Failed to parse master version: {}", version_str);
                            continue; // Skip this release
                        }
                    }
                } else {
                    tracing::warn!("Master release found without concrete version, skipping");
                    continue; // Skip master releases without concrete versions
                }
            } else {
                // Try to parse as semver version
                match semver::Version::parse(&version_key) {
                    Ok(version) => ResolvedZigVersion::Semver(version),
                    Err(_) => {
                        tracing::warn!("Failed to parse version key: {}", version_key);
                        continue; // Skip this release
                    }
                }
            };

            // Convert network artifacts to runtime artifacts
            let mut runtime_artifacts = HashMap::new();
            for (target_key, network_artifact) in network_release.targets {
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

            let runtime_release = ZigRelease::new(
                resolved_version.clone(),
                network_release.date,
                runtime_artifacts,
            );

            releases.insert(resolved_version, runtime_release);
        }

        ZigIndex::with_releases(releases, Some(chrono::Utc::now()))
    }
}

impl From<ZigIndex> for CacheZigIndex {
    fn from(runtime_index: ZigIndex) -> Self {
        let mut cache_releases = Vec::new();

        for (resolved_version, runtime_release) in runtime_index.releases {
            // Convert ResolvedZigVersion to string for cache storage
            let version_string = match &resolved_version {
                ResolvedZigVersion::Semver(v) => v.to_string(),
                ResolvedZigVersion::Master(v) => format!("master@{}", v),
            };

            // Convert runtime artifacts to cache artifacts
            let mut cache_artifacts = Vec::new();
            for (target_triple, artifact_info) in runtime_release.artifacts {
                let cache_artifact = CacheArtifact {
                    target: target_triple.to_key(),
                    tarball_url: artifact_info.ziglang_org_tarball,
                    shasum: artifact_info.shasum,
                    size: artifact_info.size,
                };
                cache_artifacts.push(cache_artifact);
            }

            // Sort artifacts by target for consistent output
            cache_artifacts.sort_by(|a, b| a.target.cmp(&b.target));

            let cache_release = CacheZigRelease {
                version: version_string,
                date: runtime_release.date,
                artifacts: cache_artifacts,
            };

            cache_releases.push(cache_release);
        }

        // Sort releases by version for consistent output
        cache_releases.sort_by(|a, b| a.version.cmp(&b.version));

        CacheZigIndex {
            releases: cache_releases,
            last_synced: runtime_index.last_synced,
        }
    }
}

impl From<CacheZigIndex> for ZigIndex {
    fn from(cache_index: CacheZigIndex) -> Self {
        let mut releases = BTreeMap::new();

        for cache_release in cache_index.releases {
            // Parse the version string back to ResolvedZigVersion
            let resolved_version =
                if let Some(version_str) = cache_release.version.strip_prefix("master@") {
                    match semver::Version::parse(version_str) {
                        Ok(version) => ResolvedZigVersion::Master(version),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse cached master version '{}': {}",
                                version_str,
                                e
                            );
                            continue; // Skip this release
                        }
                    }
                } else {
                    // Try to parse as semver version
                    match semver::Version::parse(&cache_release.version) {
                        Ok(version) => ResolvedZigVersion::Semver(version),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse cached version '{}': {}",
                                cache_release.version,
                                e
                            );
                            continue; // Skip this release
                        }
                    }
                };

            // Convert cache artifacts to runtime artifacts
            let mut runtime_artifacts = HashMap::new();
            for cache_artifact in cache_release.artifacts {
                if let Some(target_triple) = TargetTriple::from_key(&cache_artifact.target) {
                    let artifact_info = ArtifactInfo {
                        ziglang_org_tarball: cache_artifact.tarball_url,
                        shasum: cache_artifact.shasum,
                        size: cache_artifact.size,
                    };
                    runtime_artifacts.insert(target_triple, artifact_info);
                } else {
                    tracing::warn!(
                        "Failed to parse cached target key: {}",
                        cache_artifact.target
                    );
                }
            }

            let runtime_release = ZigRelease::new(
                resolved_version.clone(),
                cache_release.date,
                runtime_artifacts,
            );

            releases.insert(resolved_version, runtime_release);
        }

        ZigIndex::with_releases(releases, cache_index.last_synced)
    }
}
