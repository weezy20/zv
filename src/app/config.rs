//! Persistent zv.toml schema and I/O.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs as sync_fs;
use std::path::Path;

/// zv configuration stored in zv.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZvConfig {
    /// Current zv version
    pub version: String,
    /// Active Zig installation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_zig: Option<ActiveZig>,
    /// Tracked master version (local-master-zig)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_master_zig: Option<String>,
    /// Zig -> ZLS compatibility mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zls: Option<ZlsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZlsConfig {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub mappings: HashMap<String, String>,
}

/// Active Zig installation information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveZig {
    /// Version of active Zig installation
    pub version: String,
    /// Path to active Zig installation
    pub path: String,
    /// Whether this installation is from master
    pub is_master: bool,
}

/// Persistent config I/O errors.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read zv.toml: {0}")]
    ReadConfig(#[source] std::io::Error),

    #[error("Failed to write zv.toml: {0}")]
    WriteConfig(#[source] std::io::Error),

    #[error("Failed to parse zv.toml: {0}")]
    ParseConfig(#[source] toml::de::Error),
}

/// Load zv configuration from zv.toml
pub fn load_zv_config(path: &Path) -> Result<ZvConfig, ConfigError> {
    let contents = sync_fs::read_to_string(path).map_err(ConfigError::ReadConfig)?;

    toml::from_str(&contents).map_err(ConfigError::ParseConfig)
}

/// Save zv configuration to zv.toml
pub fn save_zv_config(path: &Path, config: &ZvConfig) -> Result<(), ConfigError> {
    let contents = toml::to_string_pretty(config).map_err(|e| {
        ConfigError::WriteConfig(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to serialize config: {}", e),
        ))
    })?;

    sync_fs::write(path, contents).map_err(ConfigError::WriteConfig)?;

    Ok(())
}
