use crate::{ArchiveExt, ResolvedZigVersion, Result, Shim, ZvError, app::utils::ProgressHandle};
use color_eyre::eyre::{Context, eyre};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::fs;
const TARGET: &'static str = "zv::app::toolchain";

/// An entry representing an installed Zig version
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    active_file: PathBuf,
}

impl ToolchainManager {
    pub async fn new(zv_root: impl AsRef<Path>) -> Result<Self, ZvError> {
        let zv_root = zv_root.as_ref().to_path_buf();
        let versions_path = zv_root.join("versions");
        let bin_path = zv_root.join("bin");
        let active_file = zv_root.join("active.json");

        // discover what is on disk
        let installations =
            Self::scan_installations(&versions_path).map_err(ZvError::ZvAppInitError)?;

        // load last active install
        let active_install = if active_file.is_file() {
            match fs::read(&active_file).await {
                Ok(bytes) => serde_json::from_slice(&bytes).ok(),
                Err(err) => {
                    tracing::debug!(target: TARGET,
                        "Failed to read active install file {}: {}",
                        active_file.display(),
                        err
                    );
                    None
                }
            }
        } else {
            None
        };

        let toolchain_manager = Self {
            versions_path,
            installations,
            active_install,
            bin_path,
            active_file,
        };

        Ok(toolchain_manager)
    }
    /// Scan installations in `versions_path` and return a sorted list of found [ZigInstall]s
    fn scan_installations(versions_path: &Path) -> Result<Vec<ZigInstall>> {
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
            if depth == 1 {
                if let Some(ver) = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<semver::Version>().ok())
                {
                    let zig_bin = path.join(&zig_exe);
                    if zig_bin.is_file() {
                        out.push(ZigInstall {
                            version: ver,
                            path: path.to_path_buf(),
                            is_master: false,
                        });
                    }
                }
            }

            // case 2: depth 2 inside master  ->  versions/master/0.13.0
            if depth == 2
                && path.parent().unwrap().file_name() == Some(std::ffi::OsStr::new("master"))
            {
                if let Some(ver) = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<semver::Version>().ok())
                {
                    let zig_bin = path.join(&zig_exe);
                    if zig_bin.is_file() {
                        out.push(ZigInstall {
                            version: ver,
                            path: path.to_path_buf(),
                            is_master: true,
                        });
                    }
                }
            }
        }

        out.sort_by(|a, b| a.version.cmp(&b.version));
        Ok(out)
    }

    /// Check if a specific version is installed
    pub fn is_version_installed(&self, rzv: &ResolvedZigVersion) -> Option<PathBuf> {
        let (is_master, version) = (rzv.is_master(), rzv.version());
        let base = if is_master {
            self.versions_path.join("master").join(version.to_string())
        } else {
            self.versions_path.join(version.to_string())
        };
        if !base.is_dir() {
            return None;
        }
        let zig = base.join(Shim::Zig.executable_name());
        if zig.is_file() { Some(zig) } else { None }
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
                progress_handle
                    .start(format!("Extracting {archive_name}"))
                    .await;
                let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(bytes));
                let mut ar = tar::Archive::new(xz);
                if let Err(e) = ar.unpack(&archive_tmp) {
                    progress_handle
                        .finish_with_error("Failed to extract tar.xz archive")
                        .await;
                    return Err(e.into());
                }
            }
            ArchiveExt::Zip => {
                progress_handle
                    .start(format!("Extracting {archive_name}"))
                    .await;
                let mut ar = match zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
                    Ok(ar) => ar,
                    Err(e) => {
                        progress_handle
                            .finish_with_error("Failed to open zip archive")
                            .await;
                        return Err(e.into());
                    }
                };
                for i in 0..ar.len() {
                    let mut file = match ar.by_index(i) {
                        Ok(file) => file,
                        Err(e) => {
                            progress_handle
                                .finish_with_error("Failed to read zip entry")
                                .await;
                            return Err(e.into());
                        }
                    };
                    let out = archive_tmp.join(file.name());
                    if file.is_dir() {
                        if let Err(e) = fs::create_dir_all(&out).await {
                            progress_handle
                                .finish_with_error("Failed to create directory during extraction")
                                .await;
                            return Err(e.into());
                        }
                    } else {
                        if let Some(p) = out.parent() {
                            if let Err(e) = fs::create_dir_all(p).await {
                                progress_handle
                                    .finish_with_error(
                                        "Failed to create parent directory during extraction",
                                    )
                                    .await;
                                return Err(e.into());
                            }
                        }
                        let mut w = match std::fs::File::create(&out) {
                            Ok(w) => w,
                            Err(e) => {
                                progress_handle
                                    .finish_with_error("Failed to create file during extraction")
                                    .await;
                                return Err(e.into());
                            }
                        };
                        if let Err(e) = std::io::copy(&mut file, &mut w) {
                            progress_handle
                                .finish_with_error("Failed to write file during extraction")
                                .await;
                            return Err(e.into());
                        }
                    }
                }
            }
        }
        progress_handle.finish("Extraction complete").await;
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

    /// Sets the active Zig version, updating the symlink in bin/ and writing to the active file
    pub async fn set_active_version(&mut self, rzv: &ResolvedZigVersion) -> Result<()> {
        let version = rzv.version();
        tracing::debug!(target: TARGET, %version, "Setting active version");
        let install = self
            .installations
            .iter()
            .find(|i| &i.version == version)
            .ok_or_else(|| eyre!("Version {} is not installed", version))?;

        tracing::debug!(target: TARGET, install_path = %install.path.display(), "Found installation, deploying link");
        self.deploy_active_link(install).await?;

        let json = serde_json::to_vec(&install)
            .wrap_err("Failed to serialize Zig install for active file")?;
        fs::write(&self.active_file, json).await?;
        self.active_install = Some(install.clone());

        tracing::trace!(target: TARGET, %version, "Set active Zig version");
        Ok(())
    }
    /// Sets the active Zig version, updating the symlink in bin/ and writing to the active file
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
        tracing::debug!(target: TARGET, "Deploying active link");
        self.deploy_active_link(&zig_install).await?;
        let json = serde_json::to_vec(&zig_install)
            .wrap_err("Failed to serialize Zig install for active file")?;
        fs::write(&self.active_file, json).await?;
        self.active_install = Some(zig_install.clone());
        tracing::trace!(target: TARGET, version = ?rzv.version().to_string(), "Set active Zig completed");
        Ok(())
    }
    /// Deploys or updates the symlink in bin/ to point to the given install's zig executable
    async fn deploy_active_link(&self, install: &ZigInstall) -> Result<()> {
        let zig_exe = Shim::Zig.executable_name();

        // Ensure we have the correct path to the zig executable
        // install.path should be the directory containing zig, but handle cases where it might be the zig executable itself
        let src = if install.path.file_name().and_then(|n| n.to_str()) == Some(zig_exe) {
            // install.path points to the zig executable itself, use it directly
            tracing::debug!(target: TARGET, "Install path points to executable directly: {}", install.path.display());
            install.path.clone()
        } else {
            // install.path points to the directory, join with executable name
            install.path.join(&zig_exe)
        };

        let dst = self.bin_path.join(&zig_exe);

        tracing::debug!(target: TARGET, src = %src.display(), dst = %dst.display(), "Deploying active link");

        // fast-path: already deployed correctly
        if dst.is_symlink() {
            tracing::debug!(target: TARGET, "Destination is symlink, checking target");
            if let Ok(current_target) = tokio::fs::read_link(&dst).await {
                // Resolve both paths to compare canonically
                let current_resolved = if current_target.is_absolute() {
                    current_target
                } else {
                    dst.parent().unwrap_or(&dst).join(&current_target)
                };

                // Compare the canonical forms
                if let (Ok(current_canonical), Ok(src_canonical)) =
                    (current_resolved.canonicalize(), src.canonicalize())
                {
                    if current_canonical == src_canonical {
                        tracing::debug!(target: TARGET, "Symlink already points to correct target, skipping");
                        return Ok(()); // identical target – nothing to do
                    }
                }
            }
        } else if dst.is_file() {
            tracing::debug!(target: TARGET, "Destination is file, checking if it's a hard link");
            // Handle hard links - check if they're the same file
            if let (Ok(dst_meta), Ok(src_meta)) = (dst.metadata(), src.metadata()) {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if dst_meta.ino() == src_meta.ino() && dst_meta.dev() == src_meta.dev() {
                        tracing::debug!(target: TARGET, "Hard link already points to correct target, skipping");
                        return Ok(()); // same file via hard link
                    }
                }
                #[cfg(windows)]
                {
                    // On Windows, compare file size and modified time as a heuristic
                    if dst_meta.len() == src_meta.len()
                        && dst_meta.modified().ok() == src_meta.modified().ok()
                    {
                        tracing::debug!(target: TARGET, "File appears to be correct hard link, skipping");
                        return Ok(());
                    }
                }
            }
        } else {
            tracing::debug!(target: TARGET, "Destination does not exist");
        }

        tracing::debug!(target: TARGET, "Creating bin directory and removing existing file");
        fs::create_dir_all(&self.bin_path).await?;

        // Remove existing file/symlink
        if dst.exists() || dst.is_symlink() {
            fs::remove_file(&dst).await?;
        }

        tracing::info!(
            target: TARGET,
            link = %dst.display(),
            to = %src.display(),
            "Creating Zig symlink"
        );

        #[cfg(unix)]
        tokio::fs::symlink(&src, &dst).await?;

        #[cfg(windows)]
        {
            match tokio::fs::symlink_file(&src, &dst).await {
                Ok(()) => {
                    tracing::debug!(target: TARGET, "Created symlink successfully");
                }
                Err(symlink_err) => {
                    tracing::debug!(target: TARGET, "Symlink failed: {}, trying hard link", symlink_err);
                    std::fs::hard_link(&src, &dst).wrap_err_with(|| {
                        format!(
                            "Failed to create hard link from {} to {}",
                            src.display(),
                            dst.display()
                        )
                    })?;
                    tracing::debug!(target: TARGET, "Created hard link successfully");
                }
            }
        }

        Ok(())
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
                    .map_or(false, |a| a.version == i.version);
                (i.version.clone(), active, i.is_master)
            })
            .collect()
    }
}
