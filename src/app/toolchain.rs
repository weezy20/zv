use crate::{ArchiveExt, ResolvedZigVersion, Result, Shim, ZvError};
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
        let active_file = zv_root.join("active");

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

        let mut toolchain_manager = Self {
            versions_path,
            installations,
            active_install,
            bin_path,
            active_file,
        };

        // ensure the active compiler is reachable in bin/
        if let Some(ref install) = toolchain_manager.active_install {
            toolchain_manager.deploy_active_link(install).await?;
        }

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

        // --- 1.  decide final destination -----------------------------------------------------------
        let dest = if is_master {
            self.versions_path.join("master").join(version.to_string())
        } else {
            self.versions_path.join(version.to_string())
        };
        tracing::debug!(
            target: TARGET,
            %version,
            is_master,
            dest = %dest.display(),
            "Determined installation destination"
        );

        // --- 2.  prepare temporary extraction directory --------------------------------------------
        let temp_dest = dest.with_extension("tmp");
        if temp_dest.exists() {
            tracing::trace!(target: TARGET, temp = %temp_dest.display(), "Removing stale temp directory");
            fs::remove_dir_all(&temp_dest).await?;
        }
        fs::create_dir_all(&temp_dest).await?;

        // --- 3.  extract archive --------------------------------------------------------------------
        let bytes = fs::read(archive_path).await?;
        tracing::trace!(
            target: TARGET,
            archive = %archive_path.display(),
            size = bytes.len(),
            "Read archive into memory"
        );

        match ext {
            ArchiveExt::TarXz => {
                tracing::trace!(target: TARGET, "Unpacking tar.xz archive");
                let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(bytes));
                let mut ar = tar::Archive::new(xz);
                ar.unpack(&temp_dest)?;
            }
            ArchiveExt::Zip => {
                tracing::trace!(target: TARGET, "Unpacking zip archive");
                let mut ar = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
                for i in 0..ar.len() {
                    let mut file = ar.by_index(i)?;
                    let out = temp_dest.join(file.name());
                    if file.is_dir() {
                        fs::create_dir_all(&out).await?;
                    } else {
                        if let Some(p) = out.parent() {
                            fs::create_dir_all(p).await?;
                        }
                        let mut w = std::fs::File::create(&out)?;
                        std::io::copy(&mut file, &mut w)?;
                    }
                }
            }
        }

        /// --- 4.  find the real root directory (strip single top-level wrapper) ----------------------
        use tokio::fs;
        let mut entries = fs::read_dir(&temp_dest).await?;
        let mut top_dirs = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                top_dirs.push(entry.path());
            }
        }

        let actual_root = match top_dirs.len() {
            1 => top_dirs.into_iter().next().unwrap(), // upstream wrapper directory
            _ => temp_dest.clone(),                    // already flat
        };

        // --- 5.  sanity-check -----------------------------------------------------------------------
        let zig_bin = actual_root.join(Shim::Zig.executable_name());
        if !zig_bin.is_file() {
            tracing::warn!(
                target: TARGET,
                expected = %zig_bin.display(),
                "Zig executable missing after extraction"
            );
            let _ = fs::remove_dir_all(&temp_dest).await;
            return Err(eyre!("Zig executable not found after installation"));
        }
        tracing::trace!(target: TARGET, zig_bin = %zig_bin.display(), "Extraction sanity-check passed");

        // --- 6.  remove old installation ------------------------------------------------------------
        if dest.exists() {
            tracing::info!(target: TARGET, old = %dest.display(), "Removing previous installation");
            fs::remove_dir_all(&dest).await?;
        }

        // --- 7.  atomically promote the real root into place ----------------------------------------
        if actual_root != temp_dest {
            // move wrapper directory contents up one level
            fs::rename(&actual_root, &dest).await.with_context(|| {
                format!(
                    "Failed to move {} -> {}",
                    actual_root.display(),
                    dest.display()
                )
            })?;
            // clean up now-empty temp directory
            fs::remove_dir_all(&temp_dest).await.ok();
        } else {
            fs::rename(&temp_dest, &dest).await?;
        }
        tracing::trace!(target: TARGET, to = %dest.display(), "Promoted installation to final location");

        // --- 8.  update in-memory cache -------------------------------------------------------------
        let new_install = ZigInstall {
            version: version.clone(),
            path: dest.clone(),
            is_master,
        };
        match self
            .installations
            .binary_search_by(|i| i.version.cmp(version))
        {
            Ok(pos) => {
                tracing::debug!(target: TARGET, %version, "Replacing existing installation in cache");
                self.installations[pos] = new_install;
            }
            Err(pos) => {
                tracing::debug!(target: TARGET, %version, "Inserting new installation into cache at position {pos}");
                self.installations.insert(pos, new_install);
            }
        }

        let final_zig_bin = dest.join(Shim::Zig.executable_name());
        tracing::info!(
            target: TARGET,
            %version,
            zig_bin = %final_zig_bin.display(),
            "Installation completed successfully"
        );
        Ok(final_zig_bin)
    }

    /// Sets the active Zig version, updating the symlink in bin/ and writing to the active file
    pub async fn set_active_version(&mut self, rzv: &ResolvedZigVersion) -> Result<()> {
        let version = rzv.version();
        let install = self
            .installations
            .iter()
            .find(|i| &i.version == version)
            .ok_or_else(|| eyre!("Version {} is not installed", version))?;

        self.deploy_active_link(install).await?;

        let json = serde_json::to_vec(&install)
            .wrap_err("Failed to serialize Zig install for active file")?;
        fs::write(&self.active_file, json).await?;
        self.active_install = Some(install.clone());

        println!("✓ Set active Zig version to {}", version);
        Ok(())
    }
    /// Sets the active Zig version, updating the symlink in bin/ and writing to the active file
    /// Optionally provide the installed path to skip re-checking installation
    pub async fn set_active_version_with_path(
        &mut self,
        rzv: &ResolvedZigVersion,
        installed_path: PathBuf,
    ) -> Result<()> {
        let zig_install = ZigInstall {
            version: rzv.version().clone(),
            path: installed_path,
            is_master: rzv.is_master(),
        };
        self.deploy_active_link(&zig_install).await?;
        let json = serde_json::to_vec(&zig_install)
            .wrap_err("Failed to serialize Zig install for active file")?;
        fs::write(&self.active_file, json).await?;
        self.active_install = Some(zig_install.clone());
        println!("✓ Set active Zig version to {}", rzv.version());
        Ok(())
    }
    /// Deploys or updates the symlink in bin/ to point to the given install's zig executable
    async fn deploy_active_link(&self, install: &ZigInstall) -> Result<()> {
        let zig_exe = Shim::Zig.executable_name();
        let src = install.path.join(&zig_exe);
        let dst = self.bin_path.join(&zig_exe);

        // fast-path: already deployed
        if dst.is_file() || dst.is_symlink() {
            let current_target = tokio::fs::read_link(&dst)
                .await
                .unwrap_or_else(|_| dst.clone()); // hard-link -> return itself
            if current_target == src {
                return Ok(()); // identical target – nothing to do
            }
        }

        fs::create_dir_all(&self.bin_path).await?;
        if dst.exists() {
            fs::remove_file(&dst).await?;
        }
        #[cfg(unix)]
        tokio::fs::symlink(&src, &dst).await?;
        #[cfg(windows)]
        if tokio::fs::symlink_file(&src, &dst).await.is_err() {
            std::fs::hard_link(&src, &dst)?;
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
