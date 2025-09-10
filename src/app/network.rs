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
pub static INDEX_TTL_DAYS: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("ZV_INDEX_TTL_DAYS")
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
    /// Management layer for community-mirrors
    mirror_manager: MirrorManager,
    /// ZV_DIR
    base_path: PathBuf,
}

impl ZvNetwork {
    pub async fn new(zv_base_path: impl AsRef<Path>) -> Result<Self, ZvError> {
        let mirrors_path = zv_base_path.as_ref().join("mirrors.toml");
        let mirror_manager = MirrorManager::load(mirrors_path, CacheStrategy::RespectTtl)
            .await
            .map_err(|net_err| {
                tracing::error!(target: TARGET, "MirrorManager initialization failed: {net_err}");
                ZvError::NetworkError(net_err)
            })?;
        Ok(Self {
            base_path: zv_base_path.as_ref().to_path_buf(),
            mirror_manager,
        })
    }
}
