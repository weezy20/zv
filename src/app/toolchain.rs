use crate::app::migrations::ZvConfig;
use crate::{ArchiveExt, ResolvedZigVersion, Result, Shim, ZvError, app::utils::ProgressHandle};
use color_eyre::eyre::{Context, eyre};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
const TARGET: &str = "zv::app::toolchain";

/// An entry representing an installed Zig version
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ZigInstall {
    /// The semantic version of this installation
    pub version: semver::Version,
    /// Path to the root directory of this installation
    pub path: PathBuf,
    /// Whether this installation is from the "master" nested directory
    pub is_master: bool,
}

#[derive(Debug, Clone)]
pub struct ToolchainManager {
    versions_path: PathBuf,
    installations: Vec<ZigInstall>,
    active_install: Option<ZigInstall>,
    bin_path: PathBuf,
    pub zv_config_file: PathBuf,
}

impl ToolchainManager {
    pub async fn new(zv_root: impl AsRef<Path>) -> Result<Self, ZvError> {
        let zv_root = zv_root.as_ref().to_path_buf();
        let versions_path = zv_root.join("versions");
        let bin_path = zv_root.join("bin");
        let zv_config_file = zv_root.join("zv.toml");

        // discover what is on disk
        let installations =
            Self::scan_installations(&versions_path).map_err(ZvError::ZvAppInitError)?;

        // Helper function to find the best fallback version from installations
        let find_fallback_install = |installations: &[ZigInstall]| -> Option<ZigInstall> {
            if installations.is_empty() {
                return None;
            }

            // Prefer highest stable version over master
            let fallback = installations
                .iter()
                .filter(|i| !i.is_master)
                .max_by(|a, b| a.version.cmp(&b.version))
                .or_else(|| {
                    // If no stable versions, use highest master version
                    installations
                        .iter()
                        .filter(|i| i.is_master)
                        .max_by(|a, b| a.version.cmp(&b.version))
                })
                .cloned();

            if let Some(ref zi) = fallback {
                // Write fallback to zv.toml
                let config = ZvConfig {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    active_zig: Some(crate::app::migrations::ActiveZig {
                        version: zi.version.to_string(),
                        path: zi.path.to_string_lossy().to_string(),
                        is_master: zi.is_master,
                    }),
                    local_master_zig: None,
                };

                if let Err(e) = crate::app::migrations::save_zv_config(&zv_config_file, &config) {
                    tracing::error!(target: TARGET, "Failed to write fallback active install to file: {}", e);
                }
            }

            fallback
        };

        // load last active install from zv.toml
        let active_install = if zv_config_file.is_file() {
            match crate::app::migrations::load_zv_config(&zv_config_file) {
                Ok(config) => {
                    if let Some(active_zig) = config.active_zig {
                        // Parse version string
                        match semver::Version::parse(&active_zig.version) {
                            Ok(version) => {
                                // Verify the install exists in our installations list
                                let path = PathBuf::from(&active_zig.path);
                                let exists = installations
                                    .iter()
                                    .any(|i| i.version == version && i.path == path);

                                if exists {
                                    Some(ZigInstall {
                                        version,
                                        path,
                                        is_master: active_zig.is_master,
                                    })
                                } else {
                                    tracing::debug!(target: TARGET,
                                        "Active install from file not found in installations, using fallback"
                                    );
                                    find_fallback_install(&installations)
                                }
                            }
                            Err(err) => {
                                tracing::debug!(target: TARGET,
                                    "Failed to parse active version from config: {}, using fallback",
                                    err
                                );
                                find_fallback_install(&installations)
                            }
                        }
                    } else {
                        find_fallback_install(&installations)
                    }
                }
                Err(err) => {
                    tracing::debug!(target: TARGET,
                        "Failed to read config file {}: {}, using fallback",
                        zv_config_file.display(),
                        err
                    );
                    find_fallback_install(&installations)
                }
            }
        } else {
            find_fallback_install(&installations)
        };

        let toolchain_manager = Self {
            versions_path,
            installations,
            active_install,
            bin_path,
            zv_config_file,
        };

        Ok(toolchain_manager)
    }
    /// Scan installations in `versions_path` and return a sorted list of found [ZigInstall]s
    pub(crate) fn scan_installations(versions_path: &Path) -> Result<Vec<ZigInstall>> {
        use walkdir::WalkDir;

        let mut out = Vec::new();
        if !versions_path.is_dir() {
            return Ok(out);
        }

        let zig_exe = Shim::Zig.executable_name();

        // Walk only 2 levels deep: versions/*  or  versions/master/*
        for entry in WalkDir::new(versions_path)
            .min_depth(1)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir())
        {
            let path = entry.path();
            let depth = entry.depth();

            // case 1: depth 1 bare semver  ->  versions/0.13.0
            if depth == 1
                && let Some(ver) = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<semver::Version>().ok())
            {
                let zig_bin = path.join(zig_exe);
                if zig_bin.is_file() {
                    out.push(ZigInstall {
                        version: ver,
                        path: path.to_path_buf(),
                        is_master: false,
                    });
                }
            }

            // case 2: depth 2 inside master  ->  versions/master/0.13.0
            if depth == 2
                && path.parent().unwrap().file_name() == Some(std::ffi::OsStr::new("master"))
                && let Some(ver) = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<semver::Version>().ok())
            {
                let zig_bin = path.join(zig_exe);
                if zig_bin.is_file() {
                    out.push(ZigInstall {
                        version: ver,
                        path: path.to_path_buf(),
                        is_master: true,
                    });
                }
            }
        }

        out.sort_by(|a, b| a.version.cmp(&b.version));
        Ok(out)
    }

    /// Check if a specific version is installed
    pub fn is_version_installed(&self, rzv: &ResolvedZigVersion) -> Option<PathBuf> {
        let version = rzv.version();
        if rzv.is_master() {
            let base = self.versions_path.join("master").join(version.to_string());
            if !base.is_dir() {
                return None;
            }
            let zig = base.join(Shim::Zig.executable_name());
            if zig.is_file() {
                return Some(zig);
            }
            // For older masters that've been moved into `versions/<semver>` following the oncoming changes:
            let alt_base = self.versions_path.join(version.to_string());
            let alt_zig = alt_base.join(Shim::Zig.executable_name());
            if alt_zig.is_file() {
                return Some(alt_zig);
            }
        }
        // Else fallback to checking versions/<semver>
        let zig = self
            .versions_path
            .join(version.to_string())
            .join(Shim::Zig.executable_name());
        if zig.is_file() {
            Some(zig)
        } else {
            // Check master dir for pre-releases semver which might not trigger is_master():
            if !version.pre.is_empty() {
                let alt_base = self.versions_path.join("master").join(version.to_string());
                let alt_zig = alt_base.join(Shim::Zig.executable_name());
                if alt_zig.is_file() {
                    return Some(alt_zig);
                }
            }
            None
        }
    }

    /// Install a Zig version from a downloaded archive
    pub async fn install_version(
        &mut self,
        archive_path: &Path,
        version: &semver::Version,
        ext: ArchiveExt,
        is_master: bool,
    ) -> Result<PathBuf> {
        const TARGET: &str = "zv::toolchain";

        let install_destination = if is_master {
            self.versions_path.join("master").join(version.to_string())
        } else {
            self.versions_path.join(version.to_string())
        };
        tracing::debug!(target: TARGET, %version, is_master, dest = %install_destination.display(), "Installation destination");

        let archive_tmp = self.versions_path.join("archive_tmp");
        if archive_tmp.exists() {
            fs::remove_dir_all(&archive_tmp).await?;
        }
        fs::create_dir_all(&archive_tmp).await?;
        let progress_handle = ProgressHandle::spawn();
        let bytes = fs::read(archive_path).await?;
        let archive_name = archive_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "zig archive".to_string());
        // extract archive
        match ext {
            ArchiveExt::TarXz => {
                let _ = progress_handle
                    .start(format!("Extracting {archive_name}"))
                    .await;
                let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(bytes));
                let mut ar = tar::Archive::new(xz);
                if let Err(e) = ar.unpack(&archive_tmp) {
                    let _ = progress_handle
                        .finish_with_error("Failed to extract tar.xz archive")
                        .await;
                    return Err(e.into());
                }
            }
            ArchiveExt::Zip => {
                let _ = progress_handle
                    .start(format!("Extracting {archive_name}"))
                    .await;
                let mut ar = match zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
                    Ok(ar) => ar,
                    Err(e) => {
                        let _ = progress_handle
                            .finish_with_error("Failed to open zip archive")
                            .await;
                        return Err(e.into());
                    }
                };
                for i in 0..ar.len() {
                    let mut file = match ar.by_index(i) {
                        Ok(file) => file,
                        Err(e) => {
                            let _ = progress_handle
                                .finish_with_error("Failed to read zip entry")
                                .await;
                            return Err(e.into());
                        }
                    };
                    let out = archive_tmp.join(file.name());
                    if file.is_dir() {
                        if let Err(e) = fs::create_dir_all(&out).await {
                            let _ = progress_handle
                                .finish_with_error("Failed to create directory during extraction")
                                .await;
                            return Err(e.into());
                        }
                    } else {
                        if let Some(p) = out.parent()
                            && let Err(e) = fs::create_dir_all(p).await
                        {
                            let _ = progress_handle
                                .finish_with_error(
                                    "Failed to create parent directory during extraction",
                                )
                                .await;
                            return Err(e.into());
                        }
                        let mut w = match std::fs::File::create(&out) {
                            Ok(w) => w,
                            Err(e) => {
                                let _ = progress_handle
                                    .finish_with_error("Failed to create file during extraction")
                                    .await;
                                return Err(e.into());
                            }
                        };
                        if let Err(e) = std::io::copy(&mut file, &mut w) {
                            let _ = progress_handle
                                .finish_with_error("Failed to write file during extraction")
                                .await;
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        let _ = progress_handle.finish("Extraction complete").await;
        // strip wrapper directory
        let mut entries = fs::read_dir(&archive_tmp).await?;
        let mut top_dirs = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                top_dirs.push(entry.path());
            }
        }
        let actual_root = match top_dirs.len() {
            1 => top_dirs.into_iter().next().unwrap(), // wrapper dir
            _ => archive_tmp.clone(),                  // already flat
        };

        // --- 5.  sanity check
        let zig_bin = actual_root.join(Shim::Zig.executable_name());
        if !zig_bin.is_file() {
            let _ = fs::remove_dir_all(&archive_tmp).await;
            return Err(eyre!("Zig executable not found after installation"));
        }

        // promote to final location
        if install_destination.exists() {
            fs::remove_dir_all(&install_destination).await?;
        }

        // Move contents of actual_root, not the directory itself
        if actual_root != archive_tmp {
            // We have a wrapper directory - move its contents to the install destination
            fs::create_dir_all(&install_destination).await?;
            let mut entries = fs::read_dir(&actual_root).await?;
            while let Some(entry) = entries.next_entry().await? {
                let src = entry.path();
                let dst = install_destination.join(entry.file_name());
                fs::rename(&src, &dst).await?;
            }
            fs::remove_dir_all(&archive_tmp).await.ok();
        } else {
            // Already flat - move the entire directory
            fs::rename(&archive_tmp, &install_destination).await?;
        }

        // update cache
        let new_install = ZigInstall {
            version: version.clone(),
            path: install_destination.clone(),
            is_master,
        };

        // Update local_master_zig if this is a master version
        if is_master {
            if let Ok(mut config) = crate::app::migrations::load_zv_config(&self.zv_config_file) {
                config.local_master_zig = Some(version.to_string());
                if let Err(e) = crate::app::migrations::save_zv_config(&self.zv_config_file, &config) {
                    tracing::error!(target: TARGET, "Failed to update local_master_zig: {}", e);
                }
            } else {
                 // Try to create config if it doesn't exist
                let config = ZvConfig {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    active_zig: None,
                    local_master_zig: Some(version.to_string()),
                };
                 if let Err(e) = crate::app::migrations::save_zv_config(&self.zv_config_file, &config) {
                    tracing::error!(target: TARGET, "Failed to create config with local_master_zig: {}", e);
                }
            }
        }

        let exe_path = new_install.path.join(Shim::Zig.executable_name());
        match self
            .installations
            .binary_search_by(|i| i.version.cmp(version))
        {
            Ok(pos) => self.installations[pos] = new_install,
            Err(pos) => self.installations.insert(pos, new_install),
        }

        Ok(exe_path)
    }

    /// Sets the active Zig version, updating the shims in bin/ and writing to the active file
    pub async fn set_active_version(&mut self, rzv: &ResolvedZigVersion) -> Result<()> {
        let version = rzv.version();
        tracing::debug!(target: TARGET, %version, "Setting active version");
        let install = self
            .installations
            .iter()
            .find(|i| &i.version == version)
            .ok_or_else(|| eyre!("Version {} is not installed", version))?;

        tracing::debug!(target: TARGET, install_path = %install.path.display(), "Found installation, deploying shims");
        self.deploy_shims(install, false, false).await?;

        // Write to zv.toml - preserve local_master_zig
        let mut config = crate::app::migrations::load_zv_config(&self.zv_config_file).unwrap_or(ZvConfig {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_zig: None,
            local_master_zig: None,
        });

        config.version = env!("CARGO_PKG_VERSION").to_string();
        config.active_zig = Some(crate::app::migrations::ActiveZig {
            version: install.version.to_string(),
            path: install.path.to_string_lossy().to_string(),
            is_master: install.is_master,
        });

        crate::app::migrations::save_zv_config(&self.zv_config_file, &config)?;
        self.active_install = Some(install.clone());

        tracing::trace!(target: TARGET, %version, "Set active Zig version");
        Ok(())
    }
    /// Sets the active Zig version, updating the shims in bin/ and writing to the active file
    /// Optionally provide the installed path to skip re-checking installation
    pub async fn set_active_version_with_path(
        &mut self,
        rzv: &ResolvedZigVersion,
        installed_path: PathBuf,
    ) -> Result<()> {
        // installed_path is the full path to zig.exe, we need the directory containing it
        let install_dir = installed_path
            .parent()
            .ok_or_else(|| eyre!("Invalid installed path: {}", installed_path.display()))?
            .to_path_buf();

        tracing::debug!(target: TARGET, version = %rzv.version(), install_dir = %install_dir.display(), "Setting active version with path");
        let zig_install = ZigInstall {
            version: rzv.version().clone(),
            path: install_dir,
            is_master: rzv.is_master(),
        };
        tracing::debug!(target: TARGET, "Deploying shims");
        self.deploy_shims(&zig_install, false, false).await?;

        // Write to zv.toml - preserve local_master_zig
        let mut config = crate::app::migrations::load_zv_config(&self.zv_config_file).unwrap_or(ZvConfig {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_zig: None,
            local_master_zig: None,
        });

        config.version = env!("CARGO_PKG_VERSION").to_string();
        config.active_zig = Some(crate::app::migrations::ActiveZig {
            version: zig_install.version.to_string(),
            path: zig_install.path.to_string_lossy().to_string(),
            is_master: zig_install.is_master,
        });

        crate::app::migrations::save_zv_config(&self.zv_config_file, &config)?;
        self.active_install = Some(zig_install.clone());
        tracing::trace!(target: TARGET, version = ?rzv.version().to_string(), "Set active Zig completed");
        Ok(())
    }
    /// Validates that the zv binary exists in the bin directory
    /// Similar to setup logic - checks existence and warns about checksum mismatches but continues
    fn validate_zv_binary(&self) -> Result<PathBuf> {
        use crate::tools::files_have_same_hash;

        let zv_bin_path = self.bin_path.join(Shim::Zv.executable_name());

        // Check if zv binary exists
        if !zv_bin_path.exists() {
            return Err(eyre!(
                "zv binary not found in bin directory: {}",
                self.bin_path.display()
            ))
            .inspect_err(|_| {
                println!(
                    "Run {} or {} to synchronize zv with ZV_DIR/bin/zv",
                    yansi::Paint::cyan("zv setup"),
                    yansi::Paint::cyan("zv sync")
                )
            });
        }

        // Get current executable for comparison
        let current_exe =
            std::env::current_exe().wrap_err("Failed to get current executable path")?;

        // Compare checksums like setup does
        match files_have_same_hash(&current_exe, &zv_bin_path) {
            Ok(true) => {
                tracing::debug!(target: TARGET, zv_path = %zv_bin_path.display(), "Validated zv binary (checksum match)");
            }
            Ok(false) => {
                tracing::warn!(target: TARGET,
                    current_exe = %current_exe.display(),
                    zv_path = %zv_bin_path.display(),
                    "zv versions mismatch (checksum) - created zig/zls installations may not perform correctly. Please run `zv setup`"
                );
            }
            Err(e) => {
                tracing::warn!(target: TARGET,
                    "zv versions mismatch (checksum comparison failed: {}) - created zig/zls installations may not perform correctly. Please run `zv setup`", e
                );
            }
        }

        tracing::debug!(target: TARGET, zv_path = %zv_bin_path.display(), "Using zv binary from bin directory");
        Ok(zv_bin_path)
    }

    /// Deploys or updates the proxy shims (zig, zls) in bin/ that link to zv
    pub async fn deploy_shims(
        &self,
        install: &ZigInstall,
        skip_zv_bin_check: bool,
        quiet: bool,
    ) -> Result<()> {
        let zv_path = if !skip_zv_bin_check {
            // Validate that zv binary exists
            self.validate_zv_binary()?
        } else {
            self.bin_path.join(Shim::Zv.executable_name())
        };

        tracing::debug!(target: TARGET, install_path = %install.path.display(), "Deploying shims for installation");

        self.create_shim(&zv_path, Shim::Zig).await?;
        // TODO: ZLS support is unimplemented
        // self.create_shim(&zv_path, Shim::Zls).await?;
        if !quiet {
            tracing::info!(target: TARGET, "Successfully deployed zig version {}", install.version);
        }
        Ok(())
    }

    /// Creates a single shim (hard link or symlink) to the zv binary
    async fn create_shim(&self, zv_path: &Path, shim: Shim) -> Result<()> {
        let shim_path = self.bin_path.join(shim.executable_name());

        tracing::trace!(target: TARGET,
            shim = shim.executable_name(),
            zv_path = %zv_path.display(),
            shim_path = %shim_path.display(),
            "Creating shim"
        );

        // Check if shim already exists and points to the correct zv binary
        if self.is_valid_shim(&shim_path, zv_path)? {
            tracing::trace!(target: TARGET, "Shim {} already exists and is valid, skipping", shim.executable_name());
            return Ok(());
        }

        // Remove existing file/symlink if it exists
        if shim_path.exists() || shim_path.is_symlink() {
            fs::remove_file(&shim_path).await?;
        }

        tracing::info!(target: TARGET,
            shim = shim.executable_name(),
            "Creating shim {} -> {}",
            shim_path.display(),
            zv_path.display()
        );

        #[cfg(unix)]
        tokio::fs::symlink(zv_path, &shim_path).await?;

        #[cfg(windows)]
        {
            match tokio::fs::symlink_file(zv_path, &shim_path).await {
                Ok(()) => {
                    tracing::debug!(target: TARGET, "Created symlink successfully for {}", shim.executable_name());
                }
                Err(symlink_err) => {
                    tracing::debug!(target: TARGET, "Symlink failed for {}: {}, trying hard link", shim.executable_name(), symlink_err);
                    std::fs::hard_link(zv_path, &shim_path).wrap_err_with(|| {
                        format!(
                            "Failed to create hard link from {} to {}",
                            zv_path.display(),
                            shim_path.display()
                        )
                    })?;
                    tracing::debug!(target: TARGET, "Created hard link successfully for {}", shim.executable_name());
                }
            }
        }

        Ok(())
    }

    /// Checks if a shim file exists and points to the correct zv binary
    fn is_valid_shim(&self, shim_path: &Path, zv_path: &Path) -> Result<bool> {
        use same_file::Handle;

        if !shim_path.exists() {
            return Ok(false);
        }

        let zv_handle =
            Handle::from_path(zv_path).wrap_err("Failed to create handle for zv binary")?;

        // Check for hard links
        if let Ok(shim_handle) = Handle::from_path(shim_path)
            && shim_handle == zv_handle
        {
            return Ok(true);
        }

        // Check for symlinks
        if shim_path.is_symlink()
            && let Ok(target) = std::fs::read_link(shim_path)
        {
            let resolved_target = if target.is_absolute() {
                target
            } else {
                shim_path.parent().unwrap_or(shim_path).join(&target)
            };

            if let Ok(target_handle) = Handle::from_path(&resolved_target)
                && target_handle == zv_handle
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get the currently active installation, if any
    pub fn get_active_install(&self) -> Option<&ZigInstall> {
        self.active_install.as_ref()
    }
    /// List all installed versions, returning a tuple of (version, is_active, is_master)
    pub fn list_installations(&self) -> Vec<(semver::Version, bool, bool)> {
        self.installations
            .iter()
            .map(|i| {
                let active = self
                    .active_install
                    .as_ref()
                    .is_some_and(|a| a.version == i.version);
                (i.version.clone(), active, i.is_master)
            })
            .collect()
    }

    /// Check if there are any installed versions
    /// Returns `true`` if no installations are available, `false` otherwise.
    pub fn installations_empty(&self) -> bool {
        self.installations.is_empty()
    }
    /// Clear the active version without setting a new one
    pub fn clear_active_version(&mut self) -> Result<()> {
        // Update config to remove active zig
        if let Ok(config) = crate::app::migrations::load_zv_config(&self.zv_config_file) {
            let updated_config = ZvConfig {
                version: config.version,
                active_zig: None,
                local_master_zig: config.local_master_zig,
            };

            if let Err(e) =
                crate::app::migrations::save_zv_config(&self.zv_config_file, &updated_config)
            {
                tracing::warn!(target: TARGET, "Failed to clear active zig from config: {}", e);
                return Err(eyre!(e).wrap_err("Failed to clear active version from config").into());
            }
        } else {
            // Config file doesn't exist, create one with no active zig
            let config = ZvConfig {
                version: env!("CARGO_PKG_VERSION").to_string(),
                active_zig: None,
                local_master_zig: None,
            };

            if let Err(e) = crate::app::migrations::save_zv_config(&self.zv_config_file, &config) {
                tracing::warn!(target: TARGET, "Failed to create config file: {}", e);
                return Err(eyre!(e).wrap_err("Failed to create config file").into());
            }
        }

        self.active_install = None;
        Ok(())
    }

    /// Delete a specific installation
    pub async fn delete_install(&mut self, install: &ZigInstall) -> Result<()> {
        tracing::debug!(target: TARGET, version = %install.version, is_master = install.is_master, "Deleting installation");
        
        fs::remove_dir_all(&install.path).await.map_err(ZvError::Io)?;

        // If this was the local master tracked in config, verify if we should clear it
        if install.is_master {
             // We can check if this specific version matches local_master_zig if needed,
             // or just let the caller handle config updates.
             // But to be fully encapsulated, we should arguably handle it here.
             // However, scan_installations and finding the "right" master to delete is done by the caller logic right now.
             // Let's stick to simple deletion here and maybe expose a method to clear local master config.
        }

        // Remove from memory
        if let Some(pos) = self.installations.iter().position(|i| i == install) {
            self.installations.remove(pos);
        }

        Ok(())
    }

    /// Clean the downloads cache directory
    pub async fn clean_downloads_cache(&self) -> Result<()> {
        let downloads_path = self.versions_path.parent().unwrap().join("downloads"); // version_path is <root>/versions
        tracing::debug!(target: TARGET, path = %downloads_path.display(), "Cleaning downloads directory");

        if !downloads_path.exists() {
            return Ok(());
        }

        fs::remove_dir_all(&downloads_path).await.map_err(ZvError::Io)?;
        fs::create_dir_all(downloads_path.join("tmp")).await.map_err(ZvError::Io)?;
        Ok(())
    }

    /// Delete all installed versions
    pub async fn delete_all_versions(&mut self) -> Result<()> {
        tracing::debug!(target: TARGET, "Deleting all versions");
        
        if self.versions_path.exists() {
             fs::remove_dir_all(&self.versions_path).await.map_err(ZvError::Io)?;
             fs::create_dir(&self.versions_path).await.map_err(ZvError::Io)?;
        }
        
        self.installations.clear();
        self.active_install = None;
        
        // Also clear config? Or just let verify/init handle it?
        // Ideally we should clear active_zig from config.
        self.clear_active_version()?;

        Ok(())
    }

    /// Get the locally tracked master version string from config
    pub fn get_local_master_version(&self) -> Option<String> {
        crate::app::migrations::load_zv_config(&self.zv_config_file)
            .ok()
            .and_then(|c| c.local_master_zig)
    }

    /// Clear the locally tracked master version in config
    pub fn clear_local_master_version(&self) -> Result<()> {
         if let Ok(mut config) = crate::app::migrations::load_zv_config(&self.zv_config_file) {
             config.local_master_zig = None;
             crate::app::migrations::save_zv_config(&self.zv_config_file, &config).map_err(|e| eyre!(e).into())
         } else {
             Ok(())
         }
    }
}
