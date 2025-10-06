pub mod constants;
pub(crate) mod network;
pub(crate) mod toolchain;
pub(crate) mod utils;
use crate::app::network::{ZigDownload, ZigRelease};
use crate::app::utils::{remove_files, zig_tarball};
use crate::types::*;
mod minisign;
use crate::path_utils;
use color_eyre::eyre::{Context as _, eyre};
pub use network::CacheStrategy;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::LazyLock;
use toolchain::ToolchainManager;

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
/// Network timeout in seconds for operations
pub static FETCH_TIMEOUT_SECS: LazyLock<u64> = LazyLock::new(|| {
    std::env::var("ZV_FETCH_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15)
});
/// Maximum number of retry attempts for downloads
pub static MAX_RETRIES: LazyLock<u32> = LazyLock::new(|| {
    std::env::var("ZV_MAX_RETRIES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3)
});

impl App {
    pub fn download_cache(&self) -> &Path {
        &self.download_cache
    }
}

/// Zv App State
#[derive(Debug, Clone)]
pub struct App {
    /// <ZV_DIR> - Home for zv
    zv_base_path: PathBuf,
    /// <ZV_DIR>/bin - Binary symlink location
    bin_path: PathBuf,
    /// <ZV_DIR>/downloads -  Download cache path
    download_cache: PathBuf,
    /// <ZV_DIR>/bin/zig - Zv managed zig executable if any
    zig: Option<PathBuf>,
    /// <ZV_DIR>/bin/zls - Zv managed zls executable if any
    #[allow(dead_code)]
    zls: Option<PathBuf>,
    /// <ZV_DIR>/versions - Installed versions
    pub(crate) versions_path: PathBuf,
    /// <ZV_DIR>/env for *nix. For powershell/cmd prompt we rely on direct PATH variable manipulation.
    env_path: PathBuf,
    /// Network client
    network: Option<network::ZvNetwork>,
    /// Toolchain manager
    pub(crate) toolchain_manager: ToolchainManager,
    /// <ZV_DIR>/bin in $PATH? If not prompt user to run `setup` or add `source <ZV_DIR>/env to their shell profile`
    pub(crate) source_set: bool,
    /// Current detected shell
    pub(crate) shell: Option<crate::Shell>,
    /// ZigRelease to install - set during resolution phase
    pub(crate) to_install: Option<ZigRelease>,
}

impl App {
    /// Minimal App path initialization & directory creation
    pub async fn init(
        UserConfig {
            zv_base_path,
            shell,
        }: UserConfig,
    ) -> Result<Self, ZvError> {
        /* path is canonicalized in tools::fetch_zv_dir() so we don't need to do that here */
        let bin_path = zv_base_path.join("bin");
        let download_cache = zv_base_path.as_path().join("downloads");

        if !bin_path.try_exists().unwrap_or_default() {
            std::fs::create_dir_all(&bin_path)
                .map_err(ZvError::Io)
                .wrap_err("Creation of bin directory failed")?;
        }
        let toolchain_manager = ToolchainManager::new(&zv_base_path).await?;
        // Check for existing ZV zig/zls shims in bin directory
        let zig = toolchain_manager
            .get_active_install()
            .map(|zig_install| zig_install.path.join(Shim::Zig.executable_name()));
        let zls = utils::detect_shim(&bin_path, Shim::Zls);

        let versions_path = zv_base_path.join("versions");
        if !versions_path.try_exists().unwrap_or(false) {
            std::fs::create_dir_all(&versions_path)
                .map_err(ZvError::Io)
                .wrap_err("Creation of versions directory failed")?;
        }

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
            bin_path,
            download_cache,
            env_path,
            toolchain_manager,
            zv_base_path,
            versions_path,
            shell,
            to_install: None,
        };
        Ok(app)
    }

    /// Set the active Zig version. Optionally provide the installed path to skip re-checking installation
    pub async fn set_active_version<'b>(
        &mut self,
        version: &'b ResolvedZigVersion,
        installed_path: Option<PathBuf>,
    ) -> crate::Result<()> {
        // Copy zv binary to bin directory if needed and regenerate shims
        crate::cli::sync::check_and_update_zv_binary(self, false)
            .await
            .wrap_err("Failed to update zv binary")?;

        if let Some(p) = installed_path {
            return self
                .toolchain_manager
                .set_active_version_with_path(version, p)
                .await;
        }
        self.toolchain_manager.set_active_version(version).await
    }

    /// Initialize network client if not already done
    pub async fn ensure_network(&mut self) -> Result<(), ZvError> {
        if self.network.is_none() {
            self.network = Some(
                network::ZvNetwork::new(self.zv_base_path.as_path(), self.download_cache.clone())
                    .await?,
            );
        }
        Ok(())
    }
    /// Initialize network client with mirror manager if not already done
    pub async fn ensure_network_with_mirrors(&mut self) -> Result<(), ZvError> {
        if self.network.is_none() {
            let mut net =
                network::ZvNetwork::new(self.zv_base_path.as_path(), self.download_cache.clone())
                    .await?;
            net.ensure_mirror_manager().await?;
            self.network = Some(net);
        } else if self.network.is_some() {
            self.network
                .as_mut()
                .unwrap()
                .ensure_mirror_manager()
                .await?;
        }
        Ok(())
    }

    /// Force refresh the Zig index from network
    pub async fn sync_zig_index(&mut self) -> Result<(), ZvError> {
        self.ensure_network().await?;

        if let Some(network) = self.network.as_mut() {
            network.sync_zig_index().await?;
        }

        Ok(())
    }

    /// Force refresh the community mirrors list from network
    pub async fn sync_mirrors(&mut self) -> Result<usize, ZvError> {
        self.ensure_network_with_mirrors().await?;

        if let Some(network) = self.network.as_mut() {
            return network.sync_mirrors().await;
        }

        Ok(0)
    }

    /// Get the current active Zig version
    pub fn get_active_version(&self) -> Option<ZigVersion> {
        self.toolchain_manager.get_active_install().map(|zi| {
            if zi.is_master {
                ZigVersion::Master(Some(zi.version.clone()))
            } else {
                ZigVersion::Semver(zi.version.clone())
            }
        })
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

    /// Fetch latest master and returns a [ZigRelease]
    pub async fn fetch_master_version(&mut self) -> Result<ZigRelease, ZvError> {
        self.ensure_network().await?;
        let zig_release = self
            .network
            .as_mut()
            .unwrap()
            .fetch_master_version()
            .await?;
        Ok(zig_release)
    }
    /// Fetch latest stable and returns a [ZigRelease]
    pub async fn fetch_latest_version(
        &mut self,
        cache_strategy: CacheStrategy,
    ) -> Result<ZigRelease, ZvError> {
        self.ensure_network().await?;
        let zig_release = self
            .network
            .as_mut()
            .unwrap()
            .fetch_latest_stable_version(cache_strategy)
            .await?;
        Ok(zig_release)
    }
    /// Validate if a semver version exists in the index and returns a [ZigRelease]
    pub async fn validate_semver(
        &mut self,
        version: &semver::Version,
    ) -> Result<ZigRelease, ZvError> {
        // todo!("Implement semver validation against installed versions and return early or else");
        self.ensure_network().await?;
        let zig_release = self
            .network
            .as_mut()
            .unwrap()
            .validate_semver(version)
            .await?;
        Ok(zig_release)
    }

    /// Check if version is installed returning Some(path) to zig binary if so
    #[inline]
    pub fn check_installed(&self, rzv: &ResolvedZigVersion) -> Option<PathBuf> {
        self.toolchain_manager.is_version_installed(rzv)
    }
    /// Install the current loaded `to_install` ZigRelease
    pub async fn install_release(&mut self, force_ziglang: bool) -> Result<PathBuf, ZvError> {
        const TARGET: &str = "zv::app::install_release";

        let zig_release = self.to_install.take().ok_or_else(|| {
            ZvError::ZigVersionResolveError(eyre!(
                "No ZigRelease is currently loaded for installation"
            ))
        })?;

        let semver_version = zig_release.resolved_version().version();
        let is_master = zig_release.resolved_version().is_master();
        tracing::debug!(
            target: TARGET,
            version = %semver_version,
            is_master,
            "Starting installation"
        );

        let zig_tarball = zig_tarball(semver_version, None).ok_or_else(|| {
            eyre!(
                "Could not determine tarball name for Zig version {}",
                zig_release.version_string()
            )
        })?;
        tracing::debug!(target: TARGET, tarball = %zig_tarball, "Determined tarball name");

        let ext = if zig_tarball.ends_with(".zip") {
            ArchiveExt::Zip
        } else if zig_tarball.ends_with(".tar.xz") {
            ArchiveExt::TarXz
        } else {
            unreachable!("Unknown archive extension for tarball: {}", zig_tarball)
        };
        tracing::debug!(target: TARGET, ?ext, "Detected archive format");
        if !force_ziglang {
            self.ensure_network_with_mirrors().await?;
        } else {
            self.ensure_network().await?;
        }
        let host_target = utils::host_target().ok_or_else(|| {
            eyre!(
                "Could not determine host target for Zig version {}",
                zig_release.version_string()
            )
        })?;
        tracing::debug!(target: TARGET, %host_target, "Resolved host target");

        let download_artifact = zig_release
            .target_artifact(&host_target)
            .ok_or_else(|| {
                eyre!(
                    "No download artifact found for target <{}> in release {}",
                    host_target,
                    zig_release.version_string()
                )
            })
            .map_err(ZvError::ZigNotFound)?;
        tracing::debug!(
            target: TARGET,
            artifact_url = %download_artifact.ziglang_org_tarball,
            "Selected download artifact"
        );

        let ZigDownload {
            tarball_path,
            minisig_path,
            mirror_used,
        } = if !force_ziglang {
            self.network
                .as_mut()
                .unwrap()
                .download_version(semver_version, &zig_tarball, download_artifact)
                .await?
        } else {
            tracing::trace!(target: "zv", "Using ziglang.org as download source");
            self.network
                .as_mut()
                .unwrap()
                .direct_download(
                    &download_artifact.ziglang_org_tarball,
                    &format!("{}.minisig", &download_artifact.ziglang_org_tarball),
                    &zig_tarball,
                    &download_artifact.shasum,
                    download_artifact.size,
                )
                .await?
        };
        tracing::debug!(
            target: TARGET,
            tarball = %tarball_path.display(),
            minisig = %minisig_path.display(),
            ?mirror_used,
            "Download completed"
        );

        let zig_exe = self
            .toolchain_manager
            .install_version(&tarball_path, semver_version, ext, is_master)
            .await?;
        tracing::info!(
            target: TARGET,
            version = %semver_version,
            "Toolchain installation succeeded"
        );

        remove_files(&[tarball_path.as_path(), minisig_path.as_path()]).await;
        tracing::debug!(target: TARGET, "Cleaned up temporary download files");

        Ok(zig_exe)
    }
}
