//! Migration system for zv
//!
//! Handles data migrations between different versions of zv.
//! For 0.9.0 onwards we have the following migrations:
//! - Flattening versions/master/* → versions/*
//! - Migrating active.json → zv.toml
//! - Text file for tracking master version (cache)

use color_eyre::eyre::{Context, Result, eyre};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fs as sync_fs;
use std::path::Path;
use tokio::fs;
use yansi::Paint;

/// zv configuration stored in zv.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZvConfig {
    /// Current zv version
    pub version: String,
    /// Active Zig installation (migrated from active.json)
    pub active_zig: Option<ActiveZig>,
}

/// Active Zig installation information (migrated from active.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveZig {
    /// Version of active Zig installation
    pub version: String,
    /// Path to active Zig installation
    pub path: String,
    /// Whether this installation is from master
    pub is_master: bool,
}

/// Migration errors
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Failed to read zv.toml: {0}")]
    ReadConfig(#[source] std::io::Error),

    #[error("Failed to write zv.toml: {0}")]
    WriteConfig(#[source] std::io::Error),

    #[error("Failed to parse zv.toml: {0}")]
    ParseConfig(#[source] toml::de::Error),
}

/// Check if migration is needed and perform it if so
pub async fn migrate(zv_root: &Path) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let current_version_parsed =
        Version::parse(current_version).expect("CARGO_PKG_VERSION should be valid semver");

    let zv_toml_path = zv_root.join("zv.toml");

    // Check if migration is needed
    let needs_migration = if !zv_toml_path.exists() {
        // This is true for v0.9.0 onwards where zv.toml is introduced
        tracing::debug!("zv.toml not found, migration needed");
        true
    } else {
        match load_zv_config(&zv_toml_path) {
            Ok(config) => {
                let config_version =
                    Version::parse(&config.version).unwrap_or_else(|_| Version::new(0, 8, 0));

                if config_version < current_version_parsed {
                    tracing::debug!(
                        "Config version {} < current version {}, migration needed",
                        config_version,
                        current_version
                    );
                    true
                } else {
                    tracing::debug!(
                        "Config version {} >= current version {}, no migration needed",
                        config_version,
                        current_version
                    );
                    false
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load zv.toml, will recreate: {}", e);
                true
            }
        }
    };

    if needs_migration {
        println!(
            "Performing zv {} migrations",
            Paint::green(current_version)
        );

        // Perform 0.8.0 -> 0.9.0 migration
        migrate_0_8_0_to_0_9_0(zv_root).await?;

        // Save updated config
        let config = ZvConfig {
            version: current_version.to_string(),
            active_zig: None,
        };

        save_zv_config(&zv_toml_path, &config)?;
    }

    Ok(())
}

/// Load zv configuration from zv.toml
pub fn load_zv_config(path: &Path) -> Result<ZvConfig, MigrationError> {
    let contents = sync_fs::read_to_string(path).map_err(MigrationError::ReadConfig)?;

    toml::from_str(&contents).map_err(MigrationError::ParseConfig)
}

/// Save zv configuration to zv.toml
pub fn save_zv_config(path: &Path, config: &ZvConfig) -> Result<(), MigrationError> {
    let contents = toml::to_string_pretty(config).map_err(|e| {
        MigrationError::WriteConfig(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to serialize config: {}", e),
        ))
    })?;

    sync_fs::write(path, contents).map_err(MigrationError::WriteConfig)?;

    Ok(())
}

/// Migration from 0.8.0 to 0.9.0
/// - Flattens versions/master/* → versions/*
/// - Migrates active.json → zv.toml
async fn migrate_0_8_0_to_0_9_0(zv_root: &Path) -> Result<()> {
    tracing::info!("Starting migration from 0.8.0 to 0.9.0");

    let versions_path = zv_root.join("versions");
    let master_dir = versions_path.join("master");
    let active_json_path = zv_root.join("active.json");

    // Step 1: Flatten versions/master/* → versions/*
    if master_dir.exists() {
        flatten_master_to_versions(&versions_path, &master_dir).await?;
    }

    // Step 2: Migrate active.json to zv.toml
    if active_json_path.exists() {
        migrate_active_json(&active_json_path).await?;
    }

    // Step 3: Clean up versions/master directory
    if master_dir.exists() {
        fs::remove_dir_all(&master_dir)
            .await
            .wrap_err("Failed to remove versions/master directory after migration")?;
        tracing::info!("Removed versions/master directory");
    }

    tracing::info!("Migration from 0.8.0 to 0.9.0 completed successfully");
    Ok(())
}

/// Flatten versions/master/* → versions/*
/// If same version exists in both, keep the one in versions/ and remove master's version
async fn flatten_master_to_versions(versions_path: &Path, master_dir: &Path) -> Result<()> {
    use walkdir::WalkDir;

    let zig_exe = if cfg!(windows) { "zig.exe" } else { "zig" };

    println!(
        "  {} Migrating versions from master/ directory...",
        "→".blue()
    );

    let mut migrated_count = 0;
    let mut skipped_count = 0;

    for entry in WalkDir::new(master_dir)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir())
    {
        let master_version_path = entry.path();
        let version_str = master_version_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let target_version_path = versions_path.join(version_str);
        let target_zig_path = target_version_path.join(zig_exe);

        // Check if version already exists in versions/
        if target_zig_path.is_file() {
            tracing::debug!(
                "Version {} already exists in versions/, skipping master version",
                version_str
            );
            skipped_count += 1;
            continue;
        }

        // Check if this is a valid Zig installation
        let master_zig_path = master_version_path.join(zig_exe);
        if !master_zig_path.is_file() {
            tracing::debug!(
                "Invalid Zig installation at {}, skipping",
                master_version_path.display()
            );
            skipped_count += 1;
            continue;
        }

        // Move the version directory
        tracing::debug!("Moving {} to versions/{}", version_str, version_str);
        fs::rename(master_version_path, &target_version_path)
            .await
            .wrap_err_with(|| {
                format!(
                    "Failed to move {} to {}",
                    master_version_path.display(),
                    target_version_path.display()
                )
            })?;

        migrated_count += 1;
        tracing::info!("Migrated version {}", version_str);
    }

    println!(
        "  {} Migrated {} versions{}",
        "✓".green(),
        migrated_count,
        if skipped_count > 0 {
            format!(" (skipped {} that already exist)", skipped_count)
        } else {
            String::new()
        }
    );

    Ok(())
}

/// Migrate active.json to zv.toml
/// Reads active.json and stores the active zig info in zv.toml
async fn migrate_active_json(active_json_path: &Path) -> Result<()> {
    tracing::debug!("Migrating active.json to zv.toml");

    let active_json = fs::read_to_string(active_json_path)
        .await
        .wrap_err("Failed to read active.json")?;

    #[derive(Debug, Deserialize)]
    struct LegacyZigInstall {
        version: semver::Version,
        path: std::path::PathBuf,
        #[serde(default)]
        is_master: bool,
    }

    let zig_install: LegacyZigInstall =
        serde_json::from_str(&active_json).wrap_err("Failed to parse active.json")?;

    let active_zig = ActiveZig {
        version: zig_install.version.to_string(),
        path: zig_install.path.to_string_lossy().to_string(),
        is_master: zig_install.is_master,
    };

    tracing::info!(
        "Migrated active Zig version {} (master: {}) from active.json",
        active_zig.version,
        active_zig.is_master
    );

    // Save to a temporary file to be picked up by the main migration flow
    let temp_path = active_json_path.parent().unwrap().join("zv.toml.migration");
    let temp_config = ZvConfig {
        version: "0.8.0".to_string(),
        active_zig: Some(active_zig),
    };

    let contents = toml::to_string_pretty(&temp_config)
        .map_err(|e| eyre!("Failed to serialize active zig config: {}", e))?;

    fs::write(&temp_path, contents)
        .await
        .wrap_err("Failed to write temporary zv.toml.migration")?;

    // Delete active.json
    fs::remove_file(active_json_path)
        .await
        .wrap_err("Failed to remove active.json")?;

    tracing::debug!("Removed active.json after migration");

    Ok(())
}

/// Update the master file with the given version
/// This should be called whenever fetch_master_version succeeds
pub async fn update_master_file(zv_root: &Path, version: &str) {
    let master_file_path = zv_root.join("master");

    match fs::write(&master_file_path, version).await {
        Ok(_) => {
            tracing::debug!("Updated master file with version: {}", version);
        }
        Err(e) => {
            tracing::error!("Failed to update master file: {}", e);
        }
    }
}

/// Read the current master version from the master file
/// Returns None if file doesn't exist or can't be read
pub async fn _read_master_file(zv_root: &Path) -> Option<String> {
    let master_file_path = zv_root.join("master");

    match fs::read_to_string(&master_file_path).await {
        Ok(contents) => {
            let version = contents.trim().to_string();
            if version.is_empty() {
                None
            } else {
                Some(version)
            }
        }
        Err(_) => None,
    }
}
