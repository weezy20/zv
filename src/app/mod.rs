#![allow(unused)]

mod config;
mod constants;
mod network;
mod utils;

use color_eyre::eyre::{Context as _, eyre};

use crate::tools::canonicalize;
use crate::types::*;
use crate::{Shell, path_utils};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Zv App State
#[derive(Debug, Default, Clone)]
pub struct App {
    /// <ZV_DIR> - Home for zv
    zv_base_path: PathBuf,
    /// <ZV_DIR>/bin - Binary symlink location
    bin_path: PathBuf,
    /// <ZV_DIR>/bin/zig - Zv managed zig executable if any
    zig: Option<PathBuf>,
    /// <ZV_DIR>/bin/zls - Zv managed zls executable if any
    zls: Option<PathBuf>,
    /// <ZV_DIR>/versions - Installed versions
    versions_path: PathBuf,
    /// <ZV_DIR>/config.toml - Config path
    config_path: PathBuf,
    /// <ZV_DIR>/env for *nix. For powershell/cmd prompt we rely on direct PATH variable manipulation.
    env_path: PathBuf,
    /// <ZV_DIR>/config.toml - Configuration implementation
    config: Option<config::ZvConfig>,
    /// Network client
    network: Option<network::ZvNetwork>,
    /// <ZV_DIR>/bin in $PATH? If not prompt user to run `setup` or add `source <ZV_DIR>/env to their shell profile`
    pub(crate) source_set: bool,
    /// Current detected shell
    pub(crate) shell: Option<crate::Shell>,
}

impl App {
    /// Minimal App path initialization & directory creation
    pub fn init(
        UserConfig {
            zv_base_path,
            shell,
        }: UserConfig,
    ) -> Result<Self, ZvError> {
        /* path is canonicalized in tools::fetch_zv_dir() so we don't need to do that here */
        let bin_path = zv_base_path.join("bin");

        let mut zig = None;
        let mut zls = None;

        if !bin_path.try_exists().unwrap_or_default() {
            std::fs::create_dir_all(&bin_path)
                .map_err(ZvError::Io)
                .wrap_err("Creation of bin directory failed")?;
        }

        // Check for existing ZV zig/zls shims in bin directory
        zig = utils::detect_shim(&bin_path, Shim::Zig);
        zls = utils::detect_shim(&bin_path, Shim::Zls);

        let versions_path = zv_base_path.join("versions");
        if !versions_path.try_exists().unwrap_or(false) {
            std::fs::create_dir_all(&versions_path)
                .map_err(ZvError::Io)
                .wrap_err("Creation of versions directory failed")?;
        }

        let config_path = zv_base_path.join("config.toml");

        let config = None;

        let env_path = if let Some(ref shell_type) = shell {
            zv_base_path.join(shell_type.env_file_name())
        } else {
            // In non-shell mode, it doesn't really matter what the file is
            zv_base_path.join("env")
        };

        let app = App {
            network: None,
            zig,
            zls,
            source_set: if let Some(ref shell_type) = shell {
                path_utils::check_dir_in_path_for_shell(shell_type, &bin_path)
            } else {
                path_utils::check_dir_in_path(&bin_path)
            },
            zv_base_path,
            bin_path,
            config_path,
            env_path,
            config,
            versions_path,
            shell: shell,
        };
        Ok(app)
    }

    /// Set the active Zig version
    pub async fn set_active_version(&mut self, version: ZigVersion) -> Result<ZigVersion, ZvError> {
        println!("App::set_active_version called with version: {:?}", version);
        println!("This is a placeholder implementation");

        // self.active_version = Some(version.clone());
        Ok(version)
    }

    /// Initialize network client if not already done
    pub async fn ensure_network(&mut self) -> Result<(), ZvError> {
        if self.network.is_none() {
            self.network = Some(network::ZvNetwork::new(self.zv_base_path.as_path()).await?);
        }
        Ok(())
    }

    /// Get the current active Zig version
    pub fn get_active_version(&self) -> Option<&ZigVersion> {
        // self.active_version.as_ref()
        None
    }

    /// Get the app's base path
    pub fn path(&self) -> &PathBuf {
        &self.zv_base_path
    }

    /// Get the app's bin path
    pub fn bin_path(&self) -> &PathBuf {
        &self.bin_path
    }

    /// Get the environment file path
    pub fn env_path(&self) -> &PathBuf {
        &self.env_path
    }

    /// Path to zv zig binary
    pub fn zv_zig(&self) -> Option<PathBuf> {
        self.zig.clone()
    }

    /// Spawn a zig process with recursion guard management
    /// Only bumps the recursion count if we're spawning our own shim
    pub(crate) fn spawn_zig_with_guard(
        &self,
        zig_path: &Path,
        args: &[&str],
        current_dir: Option<&Path>,
    ) -> Result<Output, ZvError> {
        // No need for canonicalization here, just a quick check
        let is_our_shim = zig_path.parent() == Some(self.bin_path.as_path());

        let new_count = if is_our_shim {
            let count = std::env::var("ZV_RECURSION_COUNT")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);

            let new_count = count + 1;
            tracing::trace!(
                "Spawning ZV shim zig process at {:?} with ZV_RECURSION_COUNT: {} -> {}",
                zig_path,
                count,
                new_count
            );
            Some(new_count)
        } else {
            tracing::trace!(
                "Spawning external zig process at {:?} (no recursion guard needed)",
                zig_path
            );
            None
        };

        let mut cmd = Command::new(zig_path);
        cmd.args(args);

        if let Some(dir) = current_dir {
            cmd.current_dir(dir);
        }

        if let Some(count) = new_count {
            cmd.env("ZV_RECURSION_COUNT", count.to_string());
        }

        cmd.output().map_err(|e| {
            tracing::error!(
                "Failed to execute zig at path: {:?}, error: {}",
                zig_path,
                e
            );
            ZvError::ZigExecuteError {
                source: eyre!("Failed to execute zig: {}", e),
                command: "zig ".to_string() + &args.join(" "),
            }
        })
    }

    /// Fetch a compatible ZLS version for the given Zig version
    /// This is a placeholder implementation that will be expanded with proper compatibility logic
    pub fn fetch_compatible_zls(&mut self, zig_version: &ZigVersion) -> Result<PathBuf, ZvError> {
        tracing::info!("Fetching compatible ZLS for Zig version: {:?}", zig_version);

        // Determine compatible ZLS version
        todo!()
    }

    /// Fetch latest master
    pub async fn fetch_master_version(&mut self) -> Result<ZigVersion, ZvError> {
        self.ensure_network().await?;
        self.network.as_mut().unwrap().fetch_master_version().await
    }
    /// Fetch latest stable
    pub async fn fetch_stable_version(&mut self) -> Result<ZigVersion, ZvError> {
        self.ensure_network().await?;
        // Placeholder implementation
        Ok(ZigVersion::Semver(semver::Version::new(0, 9, 1)))
    }
    /// Fetch latest release
    pub async fn fetch_latest_version(&mut self) -> Result<ZigVersion, ZvError> {
        self.ensure_network().await?;
        // Placeholder implementation
        Ok(ZigVersion::Semver(semver::Version::new(0, 10, 0)))
    }
}
