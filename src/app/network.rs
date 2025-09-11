use crate::app::utils::zv_agent;
use crate::{NetErr, ZigVersion, ZvError, tools};
use color_eyre::eyre::{Result, WrapErr, eyre};
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
    /// ZV_DIR
    base_path: PathBuf,
    /// Network Client
    client: Arc<reqwest::Client>,
}

impl ZvNetwork {
    pub async fn new(zv_base_path: impl AsRef<Path>) -> Result<Self, ZvError> {
        let client = Arc::new(
            reqwest::Client::builder()
                .user_agent(zv_agent())
                .build()
                .map_err(NetErr::Reqwest)
                .wrap_err("Failed to build HTTP client")?,
        );

        let mirrors_path = zv_base_path.as_ref().join("mirrors.toml");
        let mirror_manager = MirrorManager::init_and_load(
            mirrors_path,
            CacheStrategy::RespectTtl,
            Arc::clone(&client),
        )
        .await
        .map_err(|net_err| {
            tracing::error!(target: TARGET, "MirrorManager initialization failed: {net_err}");
            ZvError::NetworkError(net_err)
        })?;
        Ok(Self {
            client,
            base_path: zv_base_path.as_ref().to_path_buf(),
            mirror_manager,
        })
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
