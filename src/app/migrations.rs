//! On-disk layout migrations for zv.
//!
//! Handles one-shot data migrations between different versions of zv.
//! For 0.9.0 onwards we have the following migrations:
//! - Flattening versions/master/* → versions/*
//! - Migrating active.json → zv.toml
//! - Text file for tracking master version (cache)

use crate::app::config::{ActiveZig, ZvConfig, load_zv_config, save_zv_config};
use crate::app::constants::ZV_MASTER_FILE;
use color_eyre::eyre::{Context, Result};
use semver::Version;
use serde::Deserialize;
use std::fs as sync_fs;
use std::path::Path;
use tokio::fs;
use yansi::Paint;

/// Check if migration is needed and perform it if so
pub async fn migrate(zv_root: &Path, config_file: &Path) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let current_version_parsed =
        Version::parse(current_version).expect("CARGO_PKG_VERSION should be valid semver");
    let legacy_migration_version = Version::new(0, 9, 0);

    let zv_toml_path = config_file;

    let existing_config = if !zv_toml_path.exists() {
        tracing::debug!("zv.toml not found, legacy migration needed");
        None
    } else {
        match load_zv_config(&zv_toml_path) {
            Ok(config) => Some(config),
            Err(e) => {
                tracing::warn!("Failed to load zv.toml, will recreate: {}", e);
                None
            }
        }
    };

    // The only historical layout migration is the v0.8.x -> v0.9.0 move to zv.toml.
    let needs_legacy_migration = match existing_config.as_ref() {
        None => true,
        Some(config) => {
            let config_version =
                Version::parse(&config.version).unwrap_or_else(|_| Version::new(0, 8, 0));

            if config_version < legacy_migration_version {
                tracing::debug!(
                    "Config version {} < legacy migration version {}, migration needed",
                    config_version,
                    legacy_migration_version
                );
                true
            } else {
                tracing::debug!(
                    "Config version {} already has v0.9+ layout, no legacy migration needed",
                    config_version
                );
                false
            }
        }
    };

    if needs_legacy_migration {
        println!(
            "Performing zv  -> {} migrations",
            Paint::green(current_version)
        );

        // Perform 0.8.0 -> 0.9.0 migration
        let migrated_active_zig = migrate_0_8_0_to_0_9_0(zv_root).await?;

        // Save updated config with migrated active zig (if any)
        let config = ZvConfig {
            version: current_version.to_string(),
            active_zig: migrated_active_zig,
            local_master_zig: read_local_master_zig(zv_root),
            zls: None,
        };

        save_zv_config(&zv_toml_path, &config)?;
    } else if let Some(mut config) = existing_config {
        let mut changed = false;

        if let Ok(config_version) = Version::parse(&config.version) {
            if config_version < current_version_parsed {
                tracing::debug!(
                    "Updating config version {} -> {}",
                    config_version,
                    current_version
                );
                config.version = current_version.to_string();
                changed = true;
            }
        }

        if config.local_master_zig.is_none()
            && let Some(version) = read_local_master_zig(zv_root)
        {
            tracing::debug!("Migrating local_master_zig to {}", version);
            config.local_master_zig = Some(version);
            changed = true;
        }

        if changed && let Err(e) = save_zv_config(&zv_toml_path, &config) {
            tracing::error!("Failed to save migrated config: {}", e);
        }
    }

    Ok(())
}

fn read_local_master_zig(zv_root: &Path) -> Option<String> {
    let master_file = zv_root.join(ZV_MASTER_FILE);
    if !master_file.exists() {
        return None;
    }

    sync_fs::read_to_string(&master_file)
        .ok()
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty())
}

/// Migration from 0.8.0 to 0.9.0
/// - Flattens versions/master/* → versions/*
/// - Migrates active.json → zv.toml
/// Returns migrated active zig (if any)
async fn migrate_0_8_0_to_0_9_0(zv_root: &Path) -> Result<Option<ActiveZig>> {
    tracing::info!("Running v0.9.0 migrations");

    let versions_path = zv_root.join("versions");
    let master_dir = versions_path.join("master");
    let active_json_path = zv_root.join("active.json");

    let mut migrated_active_zig = None;

    // Step 1: Flatten versions/master/* → versions/*
    if master_dir.exists() {
        flatten_master_to_versions(&versions_path, &master_dir).await?;
    }

    // Step 2: Migrate active.json to zv.toml
    if active_json_path.exists() {
        migrated_active_zig = Some(migrate_active_json(&active_json_path).await?);
    }

    // Step 3: Clean up versions/master directory
    if master_dir.exists() {
        fs::remove_dir_all(&master_dir)
            .await
            .wrap_err("Failed to remove versions/master directory after migration")?;
        tracing::info!("Removed versions/master directory");
    }

    tracing::info!("Migration from 0.8.0 to 0.9.0 completed successfully");
    Ok(migrated_active_zig)
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

        // Move version directory
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

/// Migrate active.json to ActiveZig
/// Reads active.json and returns the active zig info
async fn migrate_active_json(active_json_path: &Path) -> Result<ActiveZig> {
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

    let mut path = zig_install.path;
    // If it was a master build, the path in active.json points to .../versions/master/<hash>
    // We need to update it to .../versions/<hash> as we flattened the directory
    if zig_install.is_master {
        if let Some(parent) = path.parent() {
            if parent.file_name() == Some(std::ffi::OsStr::new("master")) {
                if let Some(grandparent) = parent.parent() {
                    if let Some(file_name) = path.file_name() {
                        path = grandparent.join(file_name);
                    }
                }
            }
        }
    }

    let active_zig = ActiveZig {
        version: zig_install.version.to_string(),
        path: path.to_string_lossy().to_string(),
        is_master: zig_install.is_master,
    };

    tracing::info!(
        "Migrated active Zig version {} (master: {}) from active.json",
        active_zig.version,
        active_zig.is_master
    );

    // Delete active.json
    fs::remove_file(active_json_path)
        .await
        .wrap_err("Failed to remove active.json")?;

    tracing::debug!("Removed active.json after migration");

    Ok(active_zig)
}

/// Update the master file with the given version
/// This should be called whenever fetch_master_version succeeds
pub async fn update_master_file(master_file: &Path, version: &str) {
    match sync_fs::write(master_file, version) {
        Ok(_) => {
            tracing::debug!("Updated master file with version: {}", version);
        }
        Err(e) => {
            tracing::error!("Failed to update master file: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::ZlsConfig;
    use std::collections::HashMap;

    #[tokio::test]
    async fn preserves_v09_plus_config_when_bumping_config_version() {
        let temp = tempfile::tempdir().unwrap();
        let zv_root = temp.path();
        let config_file = zv_root.join("zv.toml");

        sync_fs::write(zv_root.join(ZV_MASTER_FILE), "0.15.1-dev\n").unwrap();

        let mut mappings = HashMap::new();
        mappings.insert("0.14.0".to_string(), "0.14.0-zls".to_string());

        save_zv_config(
            &config_file,
            &ZvConfig {
                version: "0.11.0".to_string(),
                active_zig: Some(ActiveZig {
                    version: "0.14.0".to_string(),
                    path: "/tmp/zv/versions/0.14.0".to_string(),
                    is_master: false,
                }),
                local_master_zig: None,
                zls: Some(ZlsConfig { mappings }),
            },
        )
        .unwrap();

        migrate(zv_root, &config_file).await.unwrap();

        let config = load_zv_config(&config_file).unwrap();
        assert_eq!(config.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(config.local_master_zig.as_deref(), Some("0.15.1-dev"));

        let active_zig = config.active_zig.unwrap();
        assert_eq!(active_zig.version, "0.14.0");
        assert_eq!(active_zig.path, "/tmp/zv/versions/0.14.0");
        assert!(!active_zig.is_master);

        let zls = config.zls.unwrap();
        assert_eq!(zls.mappings.get("0.14.0").unwrap(), "0.14.0-zls");
    }
}
