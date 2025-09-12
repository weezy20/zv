//! Mirrors management and types for Zig versions
//!
//! This module provides functionality for managing HTTP mirrors that host Zig releases.
//! It supports different mirror layouts (flat and versioned), caching strategies,
//! and automatic failover between mirrors.
//!
//! # Key Components
//!
//! - [`Mirror`]: Represents a single HTTP mirror with its URL and layout
//! - [`MirrorManager`]: Manages a collection of mirrors with caching and loading strategies
//! - [`MirrorsIndex`]: Cached representation of mirrors with TTL support
//! - [`Layout`]: Defines how files are organized on a mirror (flat vs versioned)
//!
//! # Cache Strategies
//!
//! The module supports three caching strategies via [`CacheStrategy`]:
//! - `AlwaysRefresh`: Always fetch fresh mirrors from network
//! - `PreferCache`: Use cache if available, fallback to network
//! - `RespectTtl`: Use cache only if not expired, otherwise refresh
//!
//! # Example Usage
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use reqwest::Client;
//! use crate::app::network::mirror::MirrorManager;
//! use crate::app::network::CacheStrategy;
//!
//! async fn example() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = Arc::new(Client::new());
//!     let cache_path = "/tmp/mirrors.toml";
//!     
//!     let mut manager = MirrorManager::init_and_load(
//!         cache_path,
//!         CacheStrategy::RespectTtl,
//!         client
//!     ).await?;
//!     
//!     let random_mirror = manager.get_random_mirror().await?;
//!     println!("Using mirror: {}", random_mirror.base_url);
//!     
//!     Ok(())
//! }
//! ```

use std::{
    convert::TryFrom,
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{CacheStrategy, MIRRORS_TTL_DAYS, TARGET};
use crate::{
    CfgErr, NetErr, ZvError,
    app::{constants::ZIG_COMMUNITY_MIRRORS, utils::zv_agent},
};
use chrono::{DateTime, Utc};
use color_eyre::eyre::{Result, WrapErr};
use rand::{Rng, prelude::IndexedRandom};
use reqwest::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use url::Url;

// ============================================================================
// LAYOUT AND MIRROR TYPES
// ============================================================================

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub enum Layout {
    /// Flat layout: {url}/{tarball}
    Flat,
    /// Versioned layout: {url}/{semver}/{tarball}
    #[default]
    Versioned,
}

impl std::ops::Not for Layout {
    type Output = Self;

    fn not(self) -> Self::Output {
        match self {
            Layout::Flat => Layout::Versioned,
            Layout::Versioned => Layout::Flat,
        }
    }
}

impl From<&str> for Layout {
    fn from(s: &str) -> Self {
        match s {
            "flat" => Layout::Flat,
            "versioned" => Layout::Versioned,
            _ => Layout::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A HTTP mirror for Zig releases
pub struct Mirror {
    pub base_url: Url,
    pub layout: Layout,
    #[serde(skip)]
    pub rank: u8,
}

// ============================================================================
// MIRROR IMPLEMENTATION
// ============================================================================

impl Mirror {
    /// Get the primary download URL based on layout
    pub fn get_download_url(&self, version: &Version, tarball: &str) -> String {
        match self.layout {
            Layout::Flat => format!(
                "{}/{tarball}?source={}",
                self.base_url.to_string(),
                zv_agent()
            ),
            Layout::Versioned => format!(
                "{}/{}/{}?source={}",
                self.base_url.to_string(),
                version,
                tarball,
                zv_agent()
            ),
        }
    }

    /// Get the download URL with layout inverted
    pub fn get_alternate_url(&self, version: &Version, tarball: &str) -> String {
        let alternate = Mirror {
            base_url: self.base_url.clone(),
            layout: !self.layout,
            rank: self.rank,
        };
        alternate.get_download_url(version, tarball)
    }
}

impl TryFrom<&str> for Mirror {
    type Error = url::ParseError;

    fn try_from(input: &str) -> Result<Self, Self::Error> {
        let url_str = if input.starts_with("http://") || input.starts_with("https://") {
            input.to_string()
        } else {
            format!("https://{input}")
        };

        let base_url = Url::parse(&url_str)?;

        // Validate scheme
        match base_url.scheme() {
            "http" | "https" => {}
            _ => return Err(url::ParseError::RelativeUrlWithoutBase),
        }
        let layout = match base_url.as_str() {
            u if u.contains("zig.florent.dev") => Layout::Flat,
            u if u.contains("zig.squirl.dev") => Layout::Flat,
            _ => Layout::Versioned,
        };

        Ok(Mirror {
            layout,
            base_url,
            rank: 1,
        })
    }
}

// ============================================================================
// MIRRORS INDEX (CACHE REPRESENTATION)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents the cached mirrors.toml file
pub struct MirrorsIndex {
    /// List of community mirrors
    pub mirrors: Vec<Mirror>,
    /// Timestamp when mirrors were last synced
    pub last_synced: DateTime<Utc>,
}

impl MirrorsIndex {
    /// Create a new index with current timestamp
    fn new(mirrors: Vec<Mirror>) -> Self {
        Self {
            mirrors,
            last_synced: Utc::now(),
        }
    }

    /// Check if the cache has expired based on TTL
    pub fn is_expired(&self) -> bool {
        self.last_synced + chrono::Duration::days(*MIRRORS_TTL_DAYS) < Utc::now()
    }

    /// Load mirrors index from disk (PreferCache strategy)
    pub async fn load_from_disk(path: impl AsRef<Path>) -> Result<Self, CfgErr> {
        let content = tokio::fs::read_to_string(path.as_ref())
            .await
            .map_err(|io_err| CfgErr::NotFound(io_err.into()))?;

        toml::from_str::<Self>(&content).map_err(|e| CfgErr::ParseFail(e.into()))
    }

    /// Load mirrors index from disk, failing if expired (RespectTtl strategy)
    pub async fn load_from_disk_expire_checked(path: impl AsRef<Path>) -> Result<Self, CfgErr> {
        let index = Self::load_from_disk(path.as_ref()).await?;

        if index.is_expired() {
            return Err(CfgErr::CacheExpired(
                path.as_ref().to_string_lossy().to_string(),
            ));
        }

        Ok(index)
    }

    /// Save mirrors index to disk
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<(), CfgErr> {
        let content = toml::to_string_pretty(self).map_err(|e| CfgErr::SerializeFail(e.into()))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|io_err| CfgErr::WriteFail(io_err.into(), "mirrors index"))?;

        Ok(())
    }
}

// ============================================================================
// MIRROR MANAGER
// ============================================================================

#[derive(Debug, Clone)]
pub struct MirrorManager {
    /// HTTP client for network requests
    client: Arc<Client>,
    /// Currently loaded mirrors
    mirrors: Vec<Mirror>,
    /// Cached mirrors index (lazy loaded)
    mirrors_index: Option<MirrorsIndex>,
    /// Path to the mirrors cache file
    cache_path: PathBuf,
}

impl MirrorManager {
    // ============================================================================
    // MIRROR MANAGER - CONSTRUCTION AND INITIALIZATION
    // ============================================================================
    /// Create a new mirror manager (doesn't load mirrors yet)
    pub fn new(cache_path: impl AsRef<Path>, client: Arc<Client>) -> Self {
        Self {
            client,
            mirrors: Vec::with_capacity(7), // 7 mirrors listed as of September 2025
            mirrors_index: None,
            cache_path: cache_path.as_ref().to_path_buf(),
        }
    }

    /// Create manager and immediately load mirrors
    pub async fn init_and_load(
        cache_path: impl AsRef<Path>,
        cache_strategy: CacheStrategy,
        client: Arc<Client>,
    ) -> Result<Self, NetErr> {
        let mut manager = Self::new(cache_path, client);
        manager.load_mirrors(cache_strategy).await?;
        Ok(manager)
    }

    // ============================================================================
    // MIRROR MANAGER - LOADING AND CACHING
    // ============================================================================
    /// Load mirrors (self.mirrors) according to the specified cache strategy
    pub async fn load_mirrors(&mut self, cache_strategy: CacheStrategy) -> Result<(), NetErr> {
        match cache_strategy {
            CacheStrategy::AlwaysRefresh => {
                self.refresh_from_network().await?;
            }
            CacheStrategy::PreferCache => {
                if self.try_load_index_from_cache().await.is_err() {
                    tracing::warn!(target: TARGET, "Failed to load cached mirrors, fetching from network");
                    self.refresh_from_network().await?;
                }
            }
            CacheStrategy::RespectTtl => match self.try_load_index_from_cache().await {
                Ok(()) => {
                    if self.is_cache_expired() {
                        tracing::debug!(target: TARGET, "Mirrors cache expired, refreshing");
                        self.refresh_from_network().await?;
                    } else {
                        tracing::debug!(target: TARGET, "Using cached mirrors");
                        self.apply_cached_mirrors_index();
                    }
                }
                Err(_) => {
                    tracing::debug!(target: TARGET, "No valid cache, fetching from network");
                    self.refresh_from_network().await?;
                }
            },
        }
        Ok(())
    }

    /// Try to load mirrors index from cache
    async fn try_load_index_from_cache(&mut self) -> Result<(), NetErr> {
        let index = MirrorsIndex::load_from_disk(&self.cache_path)
            .await
            .map_err(|err| {
                tracing::warn!(target: TARGET, "Failed to load mirrors cache from disk: {err}");
                NetErr::EmptyMirrors
            })?;

        self.mirrors_index = Some(index);
        Ok(())
    }

    /// Apply cached mirrors to active mirrors list
    fn apply_cached_mirrors_index(&mut self) {
        if let Some(ref index) = self.mirrors_index {
            self.mirrors = index.mirrors.clone();
        }
    }

    /// Refresh mirrors from network and cache them
    async fn refresh_from_network(&mut self) -> Result<(), NetErr> {
        let fresh_mirrors = self.fetch_network_mirrors().await?;
        self.mirrors = fresh_mirrors;
        let index = MirrorsIndex::new(self.mirrors.clone());

        // Save to cache (log errors but don't fail)
        if let Err(e) = index.save(&self.cache_path).await {
            tracing::error!(target: TARGET, "Failed to save mirrors cache: {}", e);
        }

        self.mirrors_index = Some(index);
        Ok(())
    }

    /// Fetch mirrors from the network
    async fn fetch_network_mirrors(&self) -> Result<Vec<Mirror>, NetErr> {
        tracing::debug!(target: TARGET, "Fetching mirrors from {}", ZIG_COMMUNITY_MIRRORS);

        let mirrors: Vec<Mirror> = self
            .client
            .get(ZIG_COMMUNITY_MIRRORS)
            .send()
            .await
            .map_err(NetErr::Reqwest)?
            .text()
            .await
            .map_err(NetErr::Reqwest)?
            .lines()
            .filter(|line| !line.trim().is_empty()) // Skip empty lines
            .filter_map(|line| {
                Mirror::try_from(line.trim())
                    .map_err(|e| {
                        tracing::warn!(target: TARGET, "Failed to parse mirror '{}': {}", line, e);
                        e
                    })
                    .ok()
            })
            .collect();

        if mirrors.is_empty() {
            tracing::error!(target: TARGET, "No valid mirrors found in response");
            return Err(NetErr::EmptyMirrors);
        }

        tracing::info!(target: TARGET, "Successfully fetched {} mirrors", mirrors.len());
        Ok(mirrors)
    }
    // ============================================================================
    // MIRROR MANAGER - INTERNAL HELPERS
    // ============================================================================
    /// Ensure mirrors are loaded (no-op if mirrors-index is already loaded)
    async fn ensure_mirrors_loaded(&mut self) -> Result<(), NetErr> {
        if self.mirrors_index.is_none() {
            match MirrorsIndex::load_from_disk(&self.cache_path).await {
                Ok(index) => {
                    self.mirrors_index = Some(index);
                }
                Err(_) => {
                    // No cache exists, fetch from network
                    self.refresh_from_network().await?;
                }
            }
        }

        // Apply mirrors from index if we don't have them loaded
        if self.mirrors.is_empty() {
            self.apply_cached_mirrors_index();
        }

        Ok(())
    }
    /// Check if the cached mirrors have expired
    #[inline]
    fn is_cache_expired(&self) -> bool {
        match &self.mirrors_index {
            Some(index) => index.is_expired(),
            None => true, // No cache loaded means it's "expired"
        }
    }
    // ============================================================================
    // MIRROR MANAGER - PUBLIC API
    // ============================================================================
    /// Get all available mirrors from MirrorManager.mirrors (loading if needed)
    pub async fn mirrors(&mut self) -> Result<&[Mirror], NetErr> {
        if self.mirrors.is_empty() {
            self.ensure_mirrors_loaded().await?;
        }
        Ok(&self.mirrors)
    }
    /// Get a random mirror for load balancing, preferring lower rank
    pub async fn get_random_mirror(&mut self) -> Result<&Mirror, NetErr> {
        use rand::Rng;

        let mirrors = self.get_ranked_mirrors().await?;

        if mirrors.is_empty() {
            return Err(NetErr::EmptyMirrors);
        }

        // If only one mirror, return it
        if mirrors.len() == 1 {
            return Ok(&mirrors[0]);
        }

        // Calculate weights inversely proportional to rank
        // Lower rank = higher weight
        let weights: Vec<f64> = mirrors
            .iter()
            .map(|m| 1.0f64 / m.rank as f64) // Rank 1 = weight 1.0, rank 2 = 0.5, rank 5 = 0.2
            .collect();

        // Simple weighted random selection
        let mut rng = rand::rng();
        let total_weight: f64 = weights.iter().sum();
        let mut random_weight = rng.random::<f64>() * total_weight;

        for (i, &weight) in weights.iter().enumerate() {
            random_weight -= weight;
            if random_weight <= 0.0 {
                return Ok(&mirrors[i]);
            }
        }

        // Fallback to first mirror (should not happen with correct weights)
        Ok(&mirrors[0])
    }
    /// Get mirrors ordered by rank
    pub async fn get_ranked_mirrors(&mut self) -> Result<Vec<&Mirror>, NetErr> {
        let mirrors = self.mirrors().await?;
        let mut ranked: Vec<&Mirror> = mirrors.iter().collect();
        ranked.sort_by_key(|m| m.rank);
        Ok(ranked)
    }
    /// Get the cache path
    pub fn cache_path(&self) -> &Path {
        &self.cache_path
    }
}
