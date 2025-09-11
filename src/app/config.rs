#![allow(unused_imports)]
use ahash::AHashMap;
use chrono::{DateTime, Utc};
use color_eyre::eyre::eyre;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use toml_edit::{DocumentMut, InlineTable, Item, Value};

use crate::ZigVersion;

const ZV_CONFIG_TARGET: &'static str = "zv_config";

/// User configuration stored in `config.toml`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZigEntry {
    /// Installation path
    pub path: PathBuf,
    /// Checksum
    #[serde(skip_serializing_if = "String::is_empty")]
    pub checksum: String,
    /// Checksum verified
    pub checksum_verified: bool,
    /// Minisig Verified
    pub minisig_verified: bool,
    /// Download URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    /// Download Timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downloaded_at: Option<DateTime<Utc>>,
}

impl ZigEntry {
    pub fn from_directory(path: PathBuf) -> Self {
        Self {
            path,
            checksum: String::new(),
            checksum_verified: false,
            minisig_verified: false,
            download_url: None,
            downloaded_at: None,
        }
    }
}

/// The configuration for zv
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZvConfig {
    /// Current active Zig Version
    pub active_version: Option<ZigVersion>,

    // /// Installed versions and their installation paths
    // #[serde(default)]
    // pub zig: AHashMap<ZigVersion, ZigEntry>,
    /// Path to the config file
    #[serde(skip)]
    pub config_path: PathBuf,
}
