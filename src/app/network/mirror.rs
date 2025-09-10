use super::TARGET;
use super::{CacheStrategy, MIRRORS_TTL_DAYS};
use crate::app::utils::zv_agent;
use crate::{NetErr, app::constants::ZIG_COMMUNITY_MIRRORS};
use chrono::{DateTime, Utc, Duration};
use color_eyre::eyre::{Result, WrapErr, eyre};
use rand::prelude::IndexedRandom;
use reqwest::Client;
use reqwest::Url;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    convert::TryFrom,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum Layout {
    /// Flat layout: {url}/{tarball}
    Flat,
    /// Versioned layout: {url}/{semver}/{tarball}
    #[default]
    Versioned,
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

impl TryFrom<&str> for Mirror {
    type Error = url::ParseError;

    fn try_from(input: &str) -> Result<Self, Self::Error> {
        // Add https:// if missing
        let url_str = if input.starts_with("http://") || input.starts_with("https://") {
            input.to_string()
        } else {
            format!("https://{}", input)
        };

        let url = Url::parse(&url_str)?;

        // Only allow http(s)
        match url.scheme() {
            "http" | "https" => {}
            _ => return Err(url::ParseError::RelativeUrlWithoutBase),
        }

        let layout = match url.as_str() {
            u if u.contains("zig.florent.dev") => Layout::Flat,
            u if u.contains("zig.squirl.dev") => Layout::Flat,

            u if u.contains("pkg.machengine.org") => Layout::Versioned,
            u if u.contains("zigmirror.hryx.net") => Layout::Versioned,
            u if u.contains("zig.linus.dev") => Layout::Versioned,
            u if u.contains("zig.mirror.mschae23.de") => Layout::Versioned,
            u if u.contains("zigmirror.meox.dev") => Layout::Versioned,
            u if u.contains("ziglang.org") => Layout::Versioned,

            _ => Layout::Versioned,
        };

        Ok(Mirror {
            url,
            layout,
            rank: 1,
        })
    }
}

impl Mirror {
    pub fn get_download_url(&self, version: &Version, tarball: &str) -> String {
        match self.layout {
            Layout::Flat => format!("{}/{}?source=zv-is-cooking", self.url, tarball),
            Layout::Versioned => {
                format!("{}/{}/{}?source=zv-is-cooking", self.url, version, tarball)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A http mirror for Zig releases
pub struct Mirror {
    pub url: Url,
    pub layout: Layout,
    #[serde(skip)]
    rank: i8,
}

#[derive(Debug, Clone)]
pub struct MirrorManager {
    /// HTTP client for network requests
    client: reqwest::Client,
    /// Active mirrors to use during runtime
    pub mirrors: Vec<Mirror>,
}

impl MirrorManager {
    pub async fn load(
        // Path to mirrors.toml cache
        cache_path: impl AsRef<Path>,
        // Cache strategy for loading mirrors list
        cache_strategy: CacheStrategy,
    ) -> Result<Self, NetErr> {
        let client = Client::builder()
            .user_agent(zv_agent())
            .build()
            .map_err(NetErr::Reqwest)
            .wrap_err("Failed to build HTTP client")?;

        let network_mirrors = async || -> Result<Vec<Mirror>, NetErr> {
            let body = client
                .get(ZIG_COMMUNITY_MIRRORS)
                .send()
                .await
                .map_err(NetErr::Reqwest)
                .wrap_err("Failed to fetch mirror list")?
                .text()
                .await
                .map_err(NetErr::Reqwest)
                .wrap_err("Failed to parse mirror list")?;

            let mirrors: Vec<Mirror> = body
                .lines()
                .map(Mirror::try_from)
                .filter_map(|result| match result {
                    Ok(mirror) => Some(mirror),
                    Err(parse_error) => {
                        tracing::error!("Failed to parse mirror URL: {}", parse_error);
                        None
                    }
                })
                .collect();

            Ok::<Vec<Mirror>, NetErr>(mirrors)
        };

        let mirrors = match cache_strategy {
            CacheStrategy::AlwaysRefresh => network_mirrors().await?,
            CacheStrategy::PreferCache => {
                todo!("load from cache if valid, else fetch from network")
            }
            CacheStrategy::RespectTtl => {
                todo!(
                    "load from cache if valid (not expired), else fetch from network and update cache"
                )
            }
        };

        if mirrors.is_empty() {
            return Err(NetErr::EmptyMirrors);
        }
        let mirrors = todo!();
        Ok(Self { client, mirrors })
    }
}

/// Represents Mirrors.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorsIndex {
    /// List of community mirrors
    pub mirrors: Vec<Mirror>,
    /// Last synced
    pub last_synced: DateTime<Utc>,
}

impl MirrorsIndex {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, NetErr> {
        Ok(Self {
            mirrors: vec![],
            last_synced: Utc::now(),
        })
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), NetErr> {
        Ok(())
    }

    pub fn is_expired(&self) -> bool {
        self.last_synced + chrono::Duration::days(*MIRRORS_TTL_DAYS) < Utc::now()
    }
}
