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
#[derive(Debug, Default)]
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
        zig = utils::detect_shim(&bin_path, utils::Shim::Zig);
        zls = utils::detect_shim(&bin_path, utils::Shim::Zls);

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
            source_set: path_utils::check_dir_in_path(&bin_path),
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
    pub(crate) fn spawn_with_guard(
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
}

// use crate::types::*;

// use ahash::AHashMap;
// use color_eyre::eyre::{Context, eyre};
// use which::which;
// mod network;
// use std::{
//     path::{Path, PathBuf},
//     process::Command,
// };
// mod config;
// use config::*;
// // use network::CacheStrategy;
// // use network::ZvNetwork;

// #[cfg(unix)]
// use std::os::unix::fs::PermissionsExt;
// #[cfg(target_os = "windows")]
// use std::os::windows::fs;
// use walkdir::WalkDir;

// pub(crate) const BUILD_CONFIG_TARGET: &'static str = "libzv::build_config";

// impl App {
//     /// Get a mutable handle to the network client, initializing if necessary
//     pub fn network_mut(&mut self) -> &mut ZvNetwork {
//         if self.network.is_none() {
//             self.network = Some(ZvNetwork::init(self.path.as_path()));
//         }
//         self.network.as_mut().unwrap()
//     }

//     /// Get shared config handle for use in other components
//     pub fn config_handle(&mut self) -> Result<Z, ZvError> {
//         if self.config.is_none() {
//             self.load_config()?;
//         }
//         Ok(self.config.as_ref().unwrap().clone())
//     }

//     /// Set zig version
//     pub async fn set_zig_version(&mut self, version: ZigVersion) -> Result<ZigVersion, ZvError> {
//         tracing::debug!(target: "libzv::app", "Setting zig version: {}", version);

//         // Load config once and check if version is already active (but not a placeholder)
//         self.load_config()?;

//         let active_version = self.config.as_ref().unwrap().get_active_version()?;
//         if let Some(active) = active_version.as_ref() {
//             // Check if we're requesting a version that's semantically the same as the active one
//             // This handles both system versions and network versions (stable, master, etc.)
//             if self.is_equivalent_version(active, &version).await? {
//                 tracing::debug!(target: "libzv::app", "Version {} already active (equivalent to {})", active, version);
//                 return Ok(active.clone());
//             }
//             // Standard equality check for exact matches
//             else if active == &version
//                 && !active.is_placeholder_version()
//                 && !version.is_placeholder_version()
//             {
//                 tracing::debug!(target: "libzv::app", "Version {} already active", version);
//                 return Ok(version);
//             }
//         }

//         tracing::info!(target: "libzv::app", "Resolving {}", version);
//         // Construct unresolved Zig instance and resolve it
//         let mut zig = Zig::new_unresolved(version);
//         self.resolve_zig(&mut zig).await?;

//         // Extract the resolved version before installing (avoids cloning)
//         let resolved_version = if zig.is_fully_resolved() {
//             // Get version from the resolved Zig instance without consuming it
//             match zig.version() {
//                 ZigVersion::System {
//                     version: Some(v), ..
//                 } => ZigVersion::System {
//                     path: zig.path().cloned(),
//                     version: Some(v.clone()),
//                 },
//                 other => other.clone(),
//             }
//         } else {
//             return Err(ZvError::ZigError(eyre!(
//                 "Zig instance was not properly resolved after resolution attempt"
//             )));
//         };

//         tracing::info!(target: "libzv::app", "Resolved to {}", resolved_version);

//         // Install the resolved Zig to the ZV bin directory
//         self.zv_install(zig)?;

//         // Update the active version in config and save it
//         self.config
//             .as_ref()
//             .unwrap()
//             .set_active_version(&resolved_version, true)?;

//         Ok(resolved_version)
//     }

//     /// Helper method to resolve a Zig instance using both SystemZig and Network traits
//     async fn resolve_zig(&mut self, zig: &mut Zig) -> Result<(), ZvError> {
//         match &zig.version() {
//             ZigVersion::System { .. } => {
//                 // For system versions, only need SystemZig functionality
//                 zig.resolve_system_only(self).await
//             }
//             _ => {
//                 // For network versions, we need to resolve the version first
//                 {
//                     let network = self.network_mut();
//                     zig.resolve_version_using_network(network).await?;
//                 }

//                 // After resolving the version, check if it exists locally
//                 // If not, download and install it to get a local path
//                 let resolved_version = zig.version().clone();
//                 if let Some(existing_path) = self.check_existing_installation(&resolved_version) {
//                     // Version already exists locally, just set the path
//                     zig.set_path(existing_path);
//                     tracing::info!(target: "libzv::app", "Found existing installation for {}", resolved_version);
//                 } else {
//                     // Version doesn't exist locally, need to download and install
//                     tracing::info!(target: "libzv::app", "Version {} not found locally, downloading...", resolved_version);
//                     let installation_path = self.download_and_install(&resolved_version).await?;
//                     zig.set_path(installation_path);
//                 }

//                 Ok(())
//             }
//         }
//     }

//     /// Modified to use ZvConfig trait with lazy loading
//     fn load_config(&mut self) -> Result<(), ZvError> {
//         match (
//             self.config.as_ref(),
//             self.config_path.try_exists().unwrap_or_default(),
//         ) {
//             (Some(_), _) => {
//                 tracing::debug!("Config already loaded");
//             }
//             (None, true) => {
//                 tracing::debug!("Loading config from {}", &self.config_path.display());
//                 let config_impl = Z::new(self.config_path.clone());
//                 self.config = Some(config_impl);
//             }
//             (None, false) => {
//                 tracing::debug!(
//                     "Config not found. Building config @ {}",
//                     &self.config_path.display()
//                 );
//                 self.build_config()?;
//             }
//         }
//         Ok(())
//     }

//     /// Build a new configuration file
//     pub fn build_config(&mut self) -> Result<(), ZvError> {
//         let system_detected: Vec<ZigVersion> = self.scan_system_zig().unwrap_or_else(Vec::new);
//         let active_version: Option<ZigVersion> = self.active_version();
//         let zv_zig: AHashMap<ZigVersion, ZigEntry> = self.scan_zv_zig();

//         // Create config data and write to file
//         let config_data = ZvConfigData {
//             active_version,
//             system_detected,
//             zig: zv_zig,
//             config_path: self.config_path.clone(),
//         };

//         let contents = toml::to_string(&config_data)
//             .map_err(|err| ZvError::ZvConfigError(CfgErr::SerializeFail(err)))?;
//         std::fs::write(&self.config_path, contents).map_err(|err| {
//             tracing::error!(
//                 target:  BUILD_CONFIG_TARGET,
//                 "Failed to write config file {}: {}",
//                 self.config_path.display(),
//                 err
//             );
//             ZvError::Io(err)
//         })?;
//         tracing::info!(
//             target:  BUILD_CONFIG_TARGET,
//             "Configuration written to: {}",
//             self.config_path.display()
//         );

//         // Initialize in-memory config
//         self.config = Some(Z::new(self.config_path.clone()));
//         Ok(())
//     }

//     pub fn ask(&self, question: &str) -> bool {
//         self.genie.ask(question)
//     }

//     /// Get the currently active zig version from config
//     pub fn get_active_version(&mut self) -> Result<Option<ZigVersion>, ZvError> {
//         if self.config.is_none() {
//             self.load_config()?;
//         }
//         self.config.as_ref().unwrap().get_active_version()
//     }

//     /// Flush current config to disk
//     pub fn save_config(&self) -> Result<(), ZvError> {
//         if let Some(config) = &self.config {
//             config.save()
//         } else {
//             Err(ZvError::ZvConfigError(CfgErr::NotFound(
//                 eyre!("No config loaded to save").into(),
//             )))
//         }
//     }

//     /// Get mutable access to the config, loading it if necessary
//     pub fn config_mut(&mut self) -> Result<&mut Z, ZvError> {
//         if self.config.is_none() {
//             self.load_config()?;
//         }
//         Ok(self.config.as_mut().unwrap())
//     }

//     /// Get immutable access to the config, loading it if necessary
//     pub fn config(&mut self) -> Result<&Z, ZvError> {
//         if self.config.is_none() {
//             self.load_config()?;
//         }
//         Ok(self.config.as_ref().unwrap())
//     }

//     /// Execute using zig in PATH
//     pub fn execute_zig(&self, args: &[&str], dir: Option<&Path>) -> Result<(), ZvError> {
//         let full_cmd = || format!("zig {}", args.join(" "));
//         if let Some(zig_exe) = self.zv_zig_or_system() {
//             // Use the active zig version
//             let status = std::process::Command::new(zig_exe)
//                 .args(args)
//                 .current_dir(dir.unwrap_or_else(|| Path::new(".")))
//                 .status()
//                 .map_err(|err| ZvError::ZigExecuteError {
//                     command: full_cmd(),
//                     source: err.into(),
//                 })
//                 .wrap_err("Failed to spawn zig command")?;

//             if !status.success() {
//                 Err(ZvError::ZigExecuteError {
//                     command: full_cmd(),
//                     source: eyre!("Cannot execute zig"),
//                 })?;
//             }
//         } else {
//             Err(ZvError::ZigExecuteError {
//                 command: full_cmd(),
//                 source: eyre!("No active zig version found. Please install a version first."),
//             })?;
//         }
//         Ok(())
//     }

//
//     /// Find all system zig in PATH excluding <ZV_DIR>/bin which works `zig version`
//     fn scan_system_zig(&self) -> Option<Vec<ZigVersion>> {
//         let zig = if cfg!(target_os = "windows") {
//             "zig.exe"
//         } else {
//             "zig"
//         };
//         let exclude_dir = self.path.as_path();
//         // We don't need to do error recovery for system zig
//         let system_paths: Vec<ZigVersion> = which::which_all(zig)
//             .map_err(|err| tracing::warn!("Couldn't find system zig paths: {}", err))
//             .ok()?
//             .filter_map(|path| {
//                 // Canonicalize both paths for proper comparison
//                 let canonical_path = path.canonicalize().ok()?;
//                 let canonical_exclude = exclude_dir.canonicalize().ok()?;

//                 // Exclude if the path is within the exclude directory
//                 (!canonical_path.starts_with(canonical_exclude)).then_some(path)
//             })
//             .filter_map(|zig| {
//                 if let Some(version) = get_zig_version(&zig).ok() {
//                     Some(ZigVersion::System {
//                         path: Some(zig),
//                         version: Some(version),
//                     })
//                 } else {
//                     None
//                 }
//             })
//             .collect();

//         (!system_paths.is_empty()).then_some(system_paths)
//     }

//     /// Get current Active zig version (this is either system zig or zv managed).
//     /// This is the one that executes on the command line.
//     /// If you just need the path use `zv_zig_or_system()`
//     fn active_version(&self) -> Option<ZigVersion> {
//         match self.zv_zig_or_system() {
//             Some(zig) => match zig.parent() {
//                 Some(parent) if parent == self.bin_path => {
//                     // This is a zv-managed zig, get version from the symlink target or executable
//                     get_zig_version(&zig).map_err(|err| {
//                         tracing::error!(target: BUILD_CONFIG_TARGET, "Failed to get version of zv managed zig for `{}`: {}", zig.display(), err);
//                         err
//                     }).ok().map(ZigVersion::Semver)
//                 }
//                 Some(_) | None => {
//                     // This is a system zig, create System variant
//                     get_zig_version(&zig).map_err(|err| {
//                         tracing::error!(target: BUILD_CONFIG_TARGET, "Failed to get version of system zig for `{}`: {}", zig.display(), err);
//                         err
//                     }).ok().map(|version| ZigVersion::System {
//                         path: Some(zig),
//                         version: Some(version)
//                     })
//                 }
//             },
//             None => None,
//         }
//     }

//     /// Scan <ZV_DIR>/versions for all active installations
//     /// We don't do any integrity checks here as it's assumed that this directory will only contain valid installations
//     fn scan_zv_zig(&self) -> AHashMap<ZigVersion, ZigEntry> {
//         let mut map = AHashMap::new();
//         for dir in WalkDir::new(&self.versions_path)
//             .max_depth(1)
//             .into_iter()
//             .filter_map(|entry| entry.ok())
//             .filter(|entry| entry.path() != self.versions_path)
//             .filter(|entry| entry.file_type().is_dir())
//         {
//             let path = dir.path();
//             let version = path
//                 .file_name()
//                 .and_then(|n| n.to_str())
//                 .ok_or_else(|| {
//                     tracing::warn!(target: BUILD_CONFIG_TARGET, "Skipping invalid directory in versions: {}", path.display());
//                 })
//                 .and_then(|v| {
//                     v.parse::<ZigVersion>().map_err(|err| {
//                         tracing::warn!(target: BUILD_CONFIG_TARGET,
//                             "Skipping directory with invalid zig version {}: {}",
//                             v,
//                             err
//                         );
//                     })
//                 })
//                 .ok();
//             if version.is_none() {
//                 continue;
//             }
//             let version = version.unwrap();
//             let entry = ZigEntry::from_directory(path.to_path_buf());
//             map.insert(version, entry);
//         }
//         map
//     }

//     /// Install a resolved Zig version to the ZV bin directory
//     pub fn zv_install(&self, zig: Zig) -> Result<(), ZvError> {
//         // Ensure the Zig is fully resolved before proceeding
//         if !zig.is_fully_resolved() || !zig.is_locally_resolved() {
//             return Err(ZvError::ZigExecuteError {
//                 command: "zv_install".to_string(),
//                 source: eyre!("Cannot install Zig: version is not fully resolved"),
//             });
//         }

//         let source_path = zig.path().expect("Already checked via resolution");

//         // Clear the bin directory completely before installing new version
//         self.clear_bin_directory()?;

//         let target_exe = if cfg!(target_os = "windows") {
//             self.bin_path.join("zig.exe")
//         } else {
//             self.bin_path.join("zig")
//         };

//         // Platform-specific installation
//         #[cfg(target_os = "windows")]
//         {
//             // On Windows, create hard link for zig.exe and junctions for directories
//             let source_dir = source_path
//                 .parent()
//                 .ok_or_else(|| ZvError::ZigExecuteError {
//                     command: "zv_install".to_string(),
//                     source: eyre!("Failed to get parent directory"),
//                 })?;

//             // Create hard link to zig.exe (doesn't require admin privileges)
//             std::fs::hard_link(source_path, &target_exe)
//                 .map_err(|err| ZvError::Io(err))
//                 .wrap_err("Failed to create hard link to zig.exe")?;

//             // Create junctions for lib and doc folders if they exist
//             let lib_src = source_dir.join("lib");
//             if lib_src.exists() && lib_src.is_dir() {
//                 let lib_dst = self.bin_path.join("lib");
//                 // Try junction first, fall back to copying if it fails
//                 if let Err(_) = std::os::windows::fs::symlink_dir(&lib_src, &lib_dst) {
//                     tracing::warn!(
//                         "Failed to create junction for lib folder, falling back to copying"
//                     );
//                     self.copy_dir_all(&lib_src, &lib_dst)
//                         .map_err(|err| ZvError::Io(err))
//                         .wrap_err("Failed to copy lib folder")?;
//                 }
//             }

//             let doc_src = source_dir.join("doc");
//             if doc_src.exists() && doc_src.is_dir() {
//                 let doc_dst = self.bin_path.join("doc");
//                 // Try junction first, fall back to copying if it fails
//                 if let Err(_) = std::os::windows::fs::symlink_dir(&doc_src, &doc_dst) {
//                     tracing::warn!(
//                         "Failed to create junction for doc folder, falling back to copying"
//                     );
//                     self.copy_dir_all(&doc_src, &doc_dst)
//                         .map_err(|err| ZvError::Io(err))
//                         .wrap_err("Failed to copy doc folder")?;
//                 }
//             }
//         }

//         #[cfg(unix)]
//         {
//             // On Unix systems, create a symlink
//             std::os::unix::fs::symlink(source_path, &target_exe).map_err(|err| ZvError::Io(err))?;
//         }

//         #[cfg(not(any(target_os = "windows", unix)))]
//         {
//             // For other platforms, copy the executable
//             std::fs::copy(source_path, &target_exe).map_err(|err| ZvError::Io(err))?;
//         }

//         tracing::info!(
//             target: "libzv::app",
//             "Successfully installed Zig {} to {}",
//             zig.version(),
//             target_exe.display()
//         );

//         Ok(())
//     }

//     /// Recursively copy a directory and all its contents to another location
//     fn copy_dir_all(&self, src: &Path, dst: &Path) -> Result<(), std::io::Error> {
//         // Create the destination directory if it doesn't exist
//         std::fs::create_dir_all(dst)?;

//         for entry in std::fs::read_dir(src)? {
//             let entry = entry?;
//             let file_type = entry.file_type()?;
//             let src_path = entry.path();
//             let dst_path = dst.join(entry.file_name());

//             if file_type.is_dir() {
//                 self.copy_dir_all(&src_path, &dst_path)?;
//             } else if file_type.is_file() {
//                 std::fs::copy(&src_path, &dst_path)?;
//             }
//         }

//         Ok(())
//     }

//     /// Clear the bin directory completely, removing all existing files and subdirectories
//     /// This ensures a clean slate before installing a new Zig version
//     fn clear_bin_directory(&self) -> Result<(), ZvError> {
//         if !self.bin_path.exists() {
//             // If bin directory doesn't exist, create it
//             std::fs::create_dir_all(&self.bin_path)
//                 .map_err(ZvError::Io)
//                 .wrap_err("Failed to create bin directory")?;
//             tracing::debug!(target: "libzv::app", "Created bin directory: {}", self.bin_path.display());
//             return Ok(());
//         }

//         tracing::debug!(target: "libzv::app", "Clearing bin directory: {}", self.bin_path.display());

//         // Remove all contents of the bin directory
//         for entry in std::fs::read_dir(&self.bin_path).map_err(ZvError::Io)? {
//             let entry = entry.map_err(ZvError::Io)?;
//             let path = entry.path();

//             if path.is_dir() {
//                 // Remove directory and all its contents
//                 std::fs::remove_dir_all(&path).map_err(|err| {
//                     tracing::warn!("Failed to remove directory {}: {}", path.display(), err);
//                     ZvError::Io(err)
//                 })?;
//                 tracing::debug!(target: "libzv::app", "Removed directory: {}", path.display());
//             } else {
//                 // Remove file
//                 std::fs::remove_file(&path).map_err(|err| {
//                     tracing::warn!("Failed to remove file {}: {}", path.display(), err);
//                     ZvError::Io(err)
//                 })?;
//                 tracing::debug!(target: "libzv::app", "Removed file: {}", path.display());
//             }
//         }

//         tracing::info!(target: "libzv::app", "Successfully cleared bin directory");
//         Ok(())
//     }

//     /// Check if two ZigVersion instances represent equivalent versions
//     /// This handles cases where one is fully specified and the other is generic
//     /// Also handles network versions (stable, master, latest) that may resolve to the same concrete version
//     async fn is_equivalent_version(
//         &mut self,
//         active: &ZigVersion,
//         requested: &ZigVersion,
//     ) -> Result<bool, ZvError> {
//         match (active, requested) {
//             // Both are system versions
//             (
//                 ZigVersion::System {
//                     path: active_path,
//                     version: active_version,
//                 },
//                 ZigVersion::System {
//                     path: requested_path,
//                     version: requested_version,
//                 },
//             ) => {
//                 match (requested_path, requested_version) {
//                     // Generic system request (path: None, version: None) - matches any active system version
//                     (None, None) => Ok(true),

//                     // Specific path requested - must match exactly
//                     (Some(req_path), _) => {
//                         if let Some(act_path) = active_path {
//                             // Compare canonical paths
//                             if let (Ok(act_canonical), Ok(req_canonical)) =
//                                 (act_path.canonicalize(), req_path.canonicalize())
//                             {
//                                 Ok(act_canonical == req_canonical)
//                             } else {
//                                 Ok(act_path == req_path)
//                             }
//                         } else {
//                             Ok(false)
//                         }
//                     }

//                     // Specific version requested - must match exactly
//                     (None, Some(req_version)) => Ok(active_version.as_ref() == Some(req_version)),
//                 }
//             }

//             // Handle network versions that might resolve to the same concrete version
//             // E.g., active: Stable(1.0.0), requested: Latest(placeholder) where latest resolves to 1.0.0
//             (active_net, requested_net)
//                 if self.are_both_network_versions(active_net, requested_net) =>
//             {
//                 // For network versions, we need to resolve the requested version and compare
//                 // the underlying semver versions
//                 match (self.get_network_version_semver(active_net), requested_net) {
//                     (Some(active_semver), requested) => {
//                         // Resolve the requested version to see what it actually points to
//                         let resolved_semver =
//                             self.resolve_network_version_to_semver(requested).await?;
//                         Ok(resolved_semver == active_semver)
//                     }
//                     _ => Ok(false),
//                 }
//             }

//             // Different version types - not equivalent
//             _ => Ok(false),
//         }
//     }

//     /// Check if both versions are network-managed versions (Master, Stable, Latest, Semver)
//     fn are_both_network_versions(&self, v1: &ZigVersion, v2: &ZigVersion) -> bool {
//         match (v1, v2) {
//             (ZigVersion::System { .. }, _) | (_, ZigVersion::System { .. }) => false,
//             (ZigVersion::Unknown, _) | (_, ZigVersion::Unknown) => false,
//             _ => true, // Master, Stable, Latest, Semver are all network-managed
//         }
//     }

//     /// Extract the semver from a resolved network version
//     fn get_network_version_semver(&self, version: &ZigVersion) -> Option<semver::Version> {
//         match version {
//             ZigVersion::Semver(v) => Some(v.clone()),
//             ZigVersion::Master(v) => Some(v.clone()),
//             ZigVersion::Stable(v) => Some(v.clone()),
//             ZigVersion::Latest(v) => Some(v.clone()),
//             _ => None,
//         }
//     }

//     /// Resolve a network version (like stable, master) to its concrete semver
//     async fn resolve_network_version_to_semver(
//         &mut self,
//         version: &ZigVersion,
//     ) -> Result<semver::Version, ZvError> {
//         match version {
//             ZigVersion::Semver(v) => Ok(v.clone()),
//             ZigVersion::Master(v)
//                 if !v.to_string().is_empty() && v != &semver::Version::new(0, 0, 0) =>
//             {
//                 Ok(v.clone())
//             }
//             ZigVersion::Stable(v)
//                 if !v.to_string().is_empty() && v != &semver::Version::new(0, 0, 0) =>
//             {
//                 Ok(v.clone())
//             }
//             ZigVersion::Latest(v)
//                 if !v.to_string().is_empty() && v != &semver::Version::new(0, 0, 0) =>
//             {
//                 Ok(v.clone())
//             }

//             // For placeholder versions, we need to resolve them
//             ZigVersion::Master(_) => {
//                 let network = self.network_mut();
//                 network.fetch_master_version().await
//             }
//             ZigVersion::Stable(_) => {
//                 let network = self.network_mut();
//                 network
//                     .fetch_latest_stable_with_strategy(CacheStrategy::PreferCache)
//                     .await
//             }
//             ZigVersion::Latest(_) => {
//                 let network = self.network_mut();
//                 network
//                     .fetch_latest_stable_with_strategy(CacheStrategy::AlwaysRefresh)
//                     .await
//             }

//             _ => Err(ZvError::ZigError(eyre!(
//                 "Cannot resolve version {} to semver",
//                 version
//             ))),
//         }
//     }

//     /// Check if a resolved version already exists in the versions directory
//     /// For versions like Master, Stable, Latest - use their resolved semver to check
//     fn check_existing_installation(&self, version: &ZigVersion) -> Option<PathBuf> {
//         // Get the installation directory name for this version
//         let dir_name = self.get_installation_dir_name(version)?;
//         let version_dir = self.versions_path.join(&dir_name);

//         if !version_dir.exists() {
//             tracing::debug!(target: "libzv::app", "Version directory {} does not exist", version_dir.display());
//             return None;
//         }

//         // Construct path to the zig executable within the version directory
//         let zig_exe = if cfg!(target_os = "windows") {
//             version_dir.join("zig.exe")
//         } else {
//             version_dir.join("zig")
//         };

//         if zig_exe.exists() {
//             tracing::debug!(target: "libzv::app", "Found existing installation: {}", zig_exe.display());
//             Some(zig_exe)
//         } else {
//             tracing::debug!(target: "libzv::app", "Version directory exists but zig executable not found: {}", zig_exe.display());
//             None
//         }
//     }

//     /// Download and install a Zig version to the versions directory
//     /// Returns the path to the installed zig executable
//     async fn download_and_install(&mut self, version: &ZigVersion) -> Result<PathBuf, ZvError> {
//         use crate::url::zig_tarball;
//         use tokio::fs as async_fs;

//         // Extract semver::Version from ZigVersion for download URL generation
//         let semver_version = match version {
//             ZigVersion::Semver(v) => v.clone(),
//             ZigVersion::Master(v) => v.clone(),
//             ZigVersion::Stable(v) => v.clone(),
//             ZigVersion::Latest(v) => v.clone(),
//             ZigVersion::System { .. } => {
//                 return Err(ZvError::General(eyre!("Cannot download system version")));
//             },
//             ZigVersion::Unknown => {
//                 return Err(ZvError::General(eyre!("Cannot download unknown version")));
//             }
//         };

//         tracing::info!(target: "libzv::download_and_install", "Starting download and install for version {} ({})", version, semver_version);

//         // Create temporary download directory
//         let tmp_dir = self.path.join("tmp");
//         if !tmp_dir.try_exists().unwrap_or_default() {
//             async_fs::create_dir_all(&tmp_dir).await
//                 .wrap_err("Failed to create temporary download directory")?;
//         }

//         // Generate platform-specific tarball name (use ZIP for Windows, tar.xz for others)
//         let use_zip = cfg!(windows);
//         let tarball_name = zig_tarball(version.clone(), use_zip)
//             .ok_or_else(|| ZvError::General(eyre!("Cannot generate tarball name for version: {}", version)))?;
//         let download_path = tmp_dir.join(&tarball_name);

//         tracing::debug!(target: "libzv::download_and_install", "Target tarball: {}, download path: {:?}", tarball_name, download_path);

//         // Get download URL using mirror system with fallback
//         let tarball_name_for_url = tarball_name.clone();

//         tracing::info!(target: "libzv::download_and_install", "Getting download URLs...");

//         // Download with mirror failover
//         self.download_with_mirror_failover(&tarball_name_for_url, &download_path).await?;

//         // Determine installation directory
//         let install_dir = self.versions_path.join(semver_version.to_string());

//         tracing::info!(target: "libzv::download_and_install", "Extracting to: {:?}", install_dir);

//         // Extract tarball to versions directory
//         self.extract_tarball(&download_path, &install_dir).await?;

//         // Clean up temporary download
//         if download_path.try_exists().unwrap_or_default() {
//             async_fs::remove_file(&download_path).await
//                 .wrap_err("Failed to clean up downloaded tarball")?;
//         }

//         // Return path to zig executable
//         // First try direct path, then look in subdirectory
//         let zig_exe_name = if cfg!(windows) { "zig.exe" } else { "zig" };
//         let mut zig_exe = install_dir.join(zig_exe_name);

//         if !zig_exe.try_exists().unwrap_or_default() {
//             // Look for zig executable in subdirectories (common with ZIP archives)
//             tracing::debug!(target: "libzv::download_and_install", "zig not found at {:?}, searching subdirectories", zig_exe);

//             if let Ok(entries) = std::fs::read_dir(install_dir) {
//                 for entry in entries {
//                     if let Ok(entry) = entry {
//                         let path = entry.path();
//                         if path.is_dir() {
//                             let candidate = path.join(zig_exe_name);
//                             if candidate.try_exists().unwrap_or_default() {
//                                 tracing::debug!(target: "libzv::download_and_install", "Found zig executable at {:?}", candidate);
//                                 zig_exe = candidate;
//                                 break;
//                             }
//                         }
//                     }
//                 }
//             }
//         }

//         if !zig_exe.try_exists().unwrap_or_default() {
//             return Err(ZvError::General(eyre!("Zig executable not found after extraction: {:?}", zig_exe)));
//         }

//         tracing::info!(target: "libzv::download_and_install", "Successfully installed version {} to {:?}", version, zig_exe);

//         Ok(zig_exe)
//     }

//     /// Download with mirror failover implementing the proper algorithm
//     async fn download_with_mirror_failover(&mut self, tarball_name: &str, download_path: &std::path::Path) -> Result<(), ZvError> {
//         use crate::constants::ZIG_BASE_DOWNLOAD_URL;

//         // Get mirrors with shuffle algorithm
//         let mut download_urls = Vec::new();

//         // First, try to get community mirrors
//         let network = self.network.get_or_insert_with(|| ZvNetwork::init(&self.path));

//         match network.get_mirrors().await {
//             Ok(mirrors_config) => {
//                 let mut mirrors = mirrors_config.mirrors.clone();

//                 // Apply shuffle_lines algorithm prioritizing low ranks
//                 crate::network::shuffle_lines(&mut mirrors);

//                 // Convert mirrors to URLs
//                 for mirror in mirrors {
//                     let url = format!("{}/{}", mirror.url.trim_end_matches('/'), tarball_name);
//                     download_urls.push(url);
//                 }

//                 tracing::info!(target: "libzv::download_with_mirror_failover", "Shuffled {} mirrors for download", download_urls.len());
//             }
//             Err(err) => {
//                 tracing::warn!(target: "libzv::download_with_mirror_failover", "Failed to get mirrors: {}", err);
//             }
//         }

//         // Always add the main download URL as fallback
//         let main_url = format!("{}/{}", ZIG_BASE_DOWNLOAD_URL.trim_end_matches('/'), tarball_name);
//         download_urls.push(main_url);

//         // Try each URL until one succeeds
//         let mut last_error = None;
//         for (index, url) in download_urls.iter().enumerate() {
//             tracing::info!(target: "libzv::download_with_mirror_failover", "Trying mirror {} of {}: {}", index + 1, download_urls.len(), url);

//             match self.download_and_verify(url, download_path).await {
//                 Ok(()) => {
//                     tracing::info!(target: "libzv::download_with_mirror_failover", "Successfully downloaded and verified from mirror {}", url);
//                     return Ok(());
//                 }
//                 Err(err) => {
//                     tracing::warn!(target: "libzv::download_with_mirror_failover", "Mirror {} failed: {}", url, err);
//                     last_error = Some(err);

//                     // TODO: Update mirror ranking based on failure type
//                     // For timeout/network errors: rank += 1
//                     // For 404/client errors: rank += 2

//                     continue;
//                 }
//             }
//         }

//         // All mirrors failed
//         Err(last_error.unwrap_or_else(|| ZvError::General(eyre!("All mirrors failed and no fallback available"))))
//     }

//     /// Get download URL for a version using mirror system with fallback
//     async fn get_download_url(&mut self, _version: &semver::Version, tarball_name: &str) -> Result<String, ZvError> {
//         use crate::constants::ZIG_BASE_DOWNLOAD_URL;

//         // First ensure we have a network instance
//         let network = self.network.get_or_insert_with(|| ZvNetwork::init(&self.path));

//         // Try to get mirrors, fall back to main URL if mirrors unavailable
//         match network.get_mirrors().await {
//             Ok(mirrors_config) => {
//                 // Sort mirrors by rank (lowest rank = highest priority)
//                 let mut sorted_mirrors = mirrors_config.mirrors.clone();
//                 sorted_mirrors.sort_by_key(|m| m.rank);

//                 // Try mirrors in rank order
//                 for mirror in &sorted_mirrors {
//                     let url = format!("{}/{}", mirror.url.trim_end_matches('/'), tarball_name);
//                     tracing::debug!(target: "libzv::get_download_url", "Trying mirror: {} (rank: {})", url, mirror.rank);

//                     // For now, return the first mirror URL
//                     // TODO: In the future, we could check mirror availability here
//                     return Ok(url);
//                 }

//                 // No mirrors available, fall back to main URL
//                 tracing::warn!(target: "libzv::get_download_url", "No mirrors available, falling back to main download URL");
//                 Ok(format!("{}/{}", ZIG_BASE_DOWNLOAD_URL.trim_end_matches('/'), tarball_name))
//             }
//             Err(err) => {
//                 tracing::warn!(target: "libzv::get_download_url", "Failed to get mirrors: {}, using main download URL", err);
//                 Ok(format!("{}/{}", ZIG_BASE_DOWNLOAD_URL.trim_end_matches('/'), tarball_name))
//             }
//         }
//     }

//     /// Download a file with retry logic and mirror ranking updates
//     async fn download_with_retry(&mut self, url: &str, download_path: &std::path::Path) -> Result<(), ZvError> {
//         const MAX_RETRIES: u32 = 3;

//         for attempt in 1..=MAX_RETRIES {
//             tracing::debug!(target: "libzv::download_with_retry", "Download attempt {} of {}", attempt, MAX_RETRIES);

//             match self.download_and_verify(url, download_path).await {
//                 Ok(()) => {
//                     tracing::info!(target: "libzv::download_with_retry", "Successfully downloaded and verified {:?}", download_path);
//                     return Ok(());
//                 }
//                 Err(err) => {
//                     tracing::warn!(target: "libzv::download_with_retry", "Download attempt {} failed: {}", attempt, err);

//                     // TODO: Update mirror ranking based on failure type
//                     // For timeout/network errors: rank += 1
//                     // For 404/client errors: rank += 2

//                     if attempt == MAX_RETRIES {
//                         return Err(err);
//                     }

//                     // Wait before retry (exponential backoff)
//                     let wait_duration = std::time::Duration::from_secs(2_u64.pow(attempt - 1));
//                     tracing::debug!(target: "libzv::download_with_retry", "Waiting {:?} before retry", wait_duration);
//                     tokio::time::sleep(wait_duration).await;
//                 }
//             }
//         }

//         unreachable!("Loop should have returned or errored")
//     }

//     /// Download a file and its signature, then verify with minisign
//     async fn download_and_verify(&mut self, url: &str, download_path: &std::path::Path) -> Result<(), ZvError> {
//         use tokio::fs as async_fs;

//         // Download the main file
//         self.download_file(url, download_path).await?;

//         // Download the signature file
//         let signature_url = format!("{}.minisig", url);
//         let signature_path = download_path.with_extension("minisig");

//         tracing::debug!(target: "libzv::download_and_verify", "Downloading signature from: {}", signature_url);
//         self.download_file(&signature_url, &signature_path).await?;

//         // Verify signature
//         tracing::debug!(target: "libzv::download_and_verify", "Verifying signature for {:?}", download_path);
//         self.verify_signature(download_path, &signature_path).await?;

//         // Clean up signature file
//         if signature_path.try_exists().unwrap_or_default() {
//             let _ = async_fs::remove_file(&signature_path).await;
//         }

//         tracing::info!(target: "libzv::download_and_verify", "Successfully verified signature for {:?}", download_path);
//         Ok(())
//     }

//     /// Verify a file's minisign signature
//     async fn verify_signature(&self, file_path: &std::path::Path, signature_path: &std::path::Path) -> Result<(), ZvError> {
//         use minisign_verify::{PublicKey, Signature};
//         use tokio::fs as async_fs;

//         // Read the public key
//         let pk = PublicKey::from_base64(crate::constants::ZIG_MINSIGN_PUBKEY)
//             .map_err(|e| ZvError::General(eyre!("Failed to parse public key: {}", e)))?;

//         // Read signature file
//         let signature_content = async_fs::read_to_string(signature_path)
//             .await
//             .wrap_err("Failed to read signature file")?;

//         let signature = Signature::decode(&signature_content)
//             .map_err(|e| ZvError::General(eyre!("Failed to decode signature: {}", e)))?;

//         // Read the file for verification
//         let file_content = async_fs::read(file_path)
//             .await
//             .wrap_err("Failed to read file for verification")?;

//         // Verify the signature - NEVER SKIP this step!
//         pk.verify(&file_content, &signature, true)
//             .map_err(|e| ZvError::General(eyre!("Signature verification failed: {}", e)))?;

//         tracing::info!(target: "libzv::verify_signature", "Signature verification successful for {:?}", file_path);
//         Ok(())
//     }

//     /// Download a file from URL to local path
//     async fn download_file(&mut self, url: &str, download_path: &std::path::Path) -> Result<(), ZvError> {
//         use tokio::fs as async_fs;
//         use tokio::io::AsyncWriteExt;
//         use reqwest::Client;

//         let client = Client::new();
//         let response = client.get(url)
//             .send()
//             .await
//             .map_err(|e| ZvError::General(eyre!("Failed to send request: {}", e)))?;

//         if !response.status().is_success() {
//             return Err(ZvError::General(eyre!("HTTP error {}: {}", response.status(), url)));
//         }

//         let mut file = async_fs::File::create(download_path)
//             .await
//             .wrap_err("Failed to create download file")?;

//         let bytes = response.bytes()
//             .await
//             .map_err(|e| ZvError::General(eyre!("Failed to read response: {}", e)))?;

//         file.write_all(&bytes)
//             .await
//             .wrap_err("Failed to write file")?;

//         file.sync_all()
//             .await
//             .wrap_err("Failed to sync file")?;

//         Ok(())
//     }

//     /// Extract tarball to installation directory
//     async fn extract_tarball(&self, tarball_path: &std::path::Path, install_dir: &std::path::Path) -> Result<(), ZvError> {
//         use tokio::fs as async_fs;
//         use std::process::Command;

//         // Create installation directory
//         if install_dir.try_exists().unwrap_or_default() {
//             async_fs::remove_dir_all(install_dir)
//                 .await
//                 .wrap_err("Failed to remove existing installation directory")?;
//         }

//         async_fs::create_dir_all(install_dir)
//             .await
//             .wrap_err("Failed to create installation directory")?;

//         // Extract based on file extension
//         let file_name = tarball_path.file_name()
//             .and_then(|n| n.to_str())
//             .ok_or_else(|| ZvError::General(eyre!("Invalid tarball filename")))?;

//         tracing::debug!(target: "libzv::extract_tarball", "Extracting {} to {:?}", file_name, install_dir);

//         if file_name.ends_with(".tar.xz") {
//             // Use tar command for .tar.xz files
//             let output = Command::new("tar")
//                 .args(["-xf", tarball_path.to_str().unwrap(), "-C", install_dir.to_str().unwrap()])
//                 .output()
//                 .map_err(|e| ZvError::General(eyre!("Failed to run tar command: {}", e)))?;

//             if !output.status.success() {
//                 let stderr = String::from_utf8_lossy(&output.stderr);
//                 return Err(ZvError::General(eyre!("tar extraction failed: {}", stderr)));
//             }
//         } else if file_name.ends_with(".zip") {
//             // Use zip crate for ZIP files
//             self.extract_zip(tarball_path, install_dir).await?;
//         } else {
//             return Err(ZvError::General(eyre!("Unsupported archive format: {}", file_name)));
//         }

//         tracing::debug!(target: "libzv::extract_tarball", "Successfully extracted tarball");
//         Ok(())
//     }

//     /// Extract ZIP file using zip crate
//     async fn extract_zip(&self, zip_path: &std::path::Path, extract_dir: &std::path::Path) -> Result<(), ZvError> {
//         use std::fs::File;
//         use std::io::copy;
//         use zip::ZipArchive;
//         use tokio::fs as async_fs;

//         tracing::debug!(target: "libzv::extract_zip", "Extracting ZIP file {:?} to {:?}", zip_path, extract_dir);

//         // Open the ZIP file
//         let file = File::open(zip_path)
//             .map_err(|e| ZvError::General(eyre!("Failed to open ZIP file: {}", e)))?;

//         let mut archive = ZipArchive::new(file)
//             .map_err(|e| ZvError::General(eyre!("Failed to read ZIP archive: {}", e)))?;

//         // Extract all files
//         for i in 0..archive.len() {
//             let mut file = archive.by_index(i)
//                 .map_err(|e| ZvError::General(eyre!("Failed to read ZIP entry {}: {}", i, e)))?;

//             let outpath = match file.enclosed_name() {
//                 Some(path) => extract_dir.join(path),
//                 None => {
//                     tracing::warn!(target: "libzv::extract_zip", "Skipping suspicious ZIP entry: {}", file.name());
//                     continue;
//                 }
//             };

//             tracing::trace!(target: "libzv::extract_zip", "Extracting: {:?}", outpath);

//             if file.is_dir() {
//                 // Create directory
//                 async_fs::create_dir_all(&outpath).await
//                     .wrap_err("Failed to create directory")?;
//             } else {
//                 // Create parent directories if needed
//                 if let Some(parent) = outpath.parent() {
//                     async_fs::create_dir_all(parent).await
//                         .wrap_err("Failed to create parent directories")?;
//                 }

//                 // Extract file
//                 let mut outfile = std::fs::File::create(&outpath)
//                     .map_err(|e| ZvError::General(eyre!("Failed to create output file {:?}: {}", outpath, e)))?;

//                 copy(&mut file, &mut outfile)
//                     .map_err(|e| ZvError::General(eyre!("Failed to extract file {:?}: {}", outpath, e)))?;
//             }

//             // Set permissions on Unix systems
//             #[cfg(unix)]
//             {
//                 use std::os::unix::fs::PermissionsExt;

//                 if let Some(mode) = file.unix_mode() {
//                     std::fs::set_permissions(&outpath, std::fs::Permissions::from_mode(mode))
//                         .map_err(|e| ZvError::General(eyre!("Failed to set permissions for {:?}: {}", outpath, e)))?;
//                 }
//             }
//         }

//         tracing::info!(target: "libzv::extract_zip", "Successfully extracted ZIP file with {} entries", archive.len());
//         Ok(())
//     }
// }

// /// Get the Semver for a zig executable at path `zig`.
// /// Returns a ZvError if Command fails to run
// pub fn get_zig_version(zig: &Path) -> Result<semver::Version, ZvError> {
//     let output = Command::new(zig)
//         .arg("version")
//         .output()
//         .map_err(|err| ZvError::Io(err))
//         .wrap_err("Failed to execute zig command")?;

//     if !output.status.success() {
//         Err(ZvError::ZigExecuteError {
//             command: format!("{} version", zig.display()),
//             source: eyre!("Zig command exited with non-zero status"),
//         })?;
//     }

//     let version_str = String::from_utf8_lossy(&output.stdout);
//     let version = version_str.trim().parse::<semver::Version>()?;

//     Ok(version)
// }

// // Implement SystemZig trait for App
// impl<G: Ask, Z: ZvConfig> SystemZig for App<G, Z> {
//     fn zv_dir(&self) -> &Path {
//         &self.path
//     }

//     fn find_system_version(&self, version: &ZigVersion) -> Result<ZigVersion, ZvError> {
//         match version {
//             ZigVersion::System {
//                 path,
//                 version: target_version,
//             } => {
//                 match (path, target_version) {
//                     // Path specified - validate and use it
//                     (Some(p), target) => {
//                         let actual_version =
//                             self.validate_system_path_and_version(p, target.as_ref())?;
//                         Ok(ZigVersion::System {
//                             path: Some(p.clone()),
//                             version: Some(actual_version),
//                         })
//                     }
//                     // No path, but version specified - search for matching system version
//                     (None, Some(target)) => self.search_matching_version(target),
//                     // Neither path nor version specified - find any system Zig
//                     (None, None) => self.find_any_system_zig(),
//                 }
//             }
//             _ => Err(ZvError::SystemZigError(eyre!(
//                 "find_system_version can only be called with ZigVersion::System variants"
//             ))),
//         }
//     }

//     fn find_latest_stable(&self) -> Result<ZigVersion, ZvError> {
//         let system_versions = self.get_system_versions()?;
//         let mut stable_versions: Vec<_> = system_versions
//             .into_iter()
//             .filter_map(|zv| {
//                 if let ZigVersion::System { path, version } = zv {
//                     if let (Some(p), Some(v)) = (path, version) {
//                         if v.pre.is_empty() && p.exists() && !self.is_zv_managed_path(&p) {
//                             Some((v, p))
//                         } else {
//                             None
//                         }
//                     } else {
//                         None
//                     }
//                 } else {
//                     None
//                 }
//             })
//             .collect();

//         stable_versions.sort_by(|a, b| a.0.cmp(&b.0));

//         if let Some((version, path)) = stable_versions.last() {
//             Ok(ZigVersion::System {
//                 path: Some(path.to_owned()),
//                 version: Some(version.to_owned()),
//             })
//         } else {
//             Err(ZvError::SystemZigError(eyre!(
//                 "No stable system Zig versions found"
//             )))
//         }
//     }

//     fn find_latest_dev(&self) -> Result<ZigVersion, ZvError> {
//         let system_versions = self.get_system_versions()?;
//         let mut dev_versions: Vec<_> = system_versions
//             .iter()
//             .filter_map(|zv| {
//                 if let ZigVersion::System { path, version } = zv {
//                     if let (Some(p), Some(v)) = (path, version) {
//                         if !v.pre.is_empty() && p.exists() && !self.is_zv_managed_path(p) {
//                             Some((v, p))
//                         } else {
//                             None
//                         }
//                     } else {
//                         None
//                     }
//                 } else {
//                     None
//                 }
//             })
//             .collect();

//         dev_versions.sort_by(|a, b| a.0.cmp(b.0));

//         if let Some((version, path)) = dev_versions.last() {
//             Ok(ZigVersion::System {
//                 path: Some((*path).clone()),
//                 version: Some((*version).clone()),
//             })
//         } else {
//             Err(ZvError::SystemZigError(eyre!(
//                 "No development system Zig versions found"
//             )))
//         }
//     }

//     fn find_exact_or_compatible(&self, target_version: &ZigVersion) -> Result<ZigVersion, ZvError> {
//         match target_version {
//             ZigVersion::System {
//                 version: Some(target),
//                 ..
//             } => self.search_matching_version(target),
//             _ => Err(ZvError::SystemZigError(eyre!(
//                 "find_exact_or_compatible requires a System version with a target version"
//             ))),
//         }
//     }

//     fn is_zv_managed_path(&self, path: &Path) -> bool {
//         if let Ok(canonical_path) = path.canonicalize() {
//             if let Ok(canonical_zv_dir) = self.path.canonicalize() {
//                 return canonical_path.starts_with(canonical_zv_dir);
//             }
//         }
//         false
//     }

//     fn exists(&self, version: &ZigVersion) -> Option<PathBuf> {
//         match version {
//             ZigVersion::System { path: Some(p), .. } => {
//                 if p.exists() && !self.is_zv_managed_path(p) {
//                     Some(p.clone())
//                 } else {
//                     None
//                 }
//             }
//             ZigVersion::System { path: None, .. } => {
//                 // System version without path - cannot determine existence
//                 None
//             }
//             // Handle zv-managed versions (Semver, Master, Stable, Latest)
//             _ => {
//                 // Get the installation directory name for this version
//                 let dir_name = self.get_installation_dir_name(version)?;
//                 let version_dir = self.versions_path.join(dir_name);

//                 if !version_dir.exists() {
//                     return None;
//                 }

//                 // Construct path to the zig executable within the version directory
//                 let zig_exe = if cfg!(target_os = "windows") {
//                     version_dir.join("zig.exe")
//                 } else {
//                     version_dir.join("zig")
//                 };

//                 if zig_exe.exists() {
//                     Some(zig_exe)
//                 } else {
//                     None
//                 }
//             }
//         }
//     }

//     async fn resolve_path(
//         &mut self,
//         path: &Option<PathBuf>,
//         version: &Option<semver::Version>,
//     ) -> Result<(), ZvError> {
//         match (path, version) {
//             (Some(p), v) => {
//                 // Use the helper method for validation (it handles both Some and None for version)
//                 self.validate_system_path_and_version(p, v.as_ref())?;
//                 Ok(())
//             }
//             (None, Some(target_version)) => {
//                 // First check system_detected in config for existing version
//                 if let Ok(system_versions) = self.get_system_versions() {
//                     for zv in &system_versions {
//                         if let ZigVersion::System {
//                             path: Some(p),
//                             version: Some(v),
//                         } = zv
//                         {
//                             if v == target_version && !self.is_zv_managed_path(p) {
//                                 // Found existing version, validate it's still accessible
//                                 self.validate_system_path_and_version(p, Some(target_version))?;
//                                 return Ok(());
//                             }
//                         }
//                     }
//                 }

//                 // Not found in config, perform fresh system scan
//                 tracing::debug!(
//                     "Version {} not found in config, performing fresh system scan",
//                     target_version
//                 );
//                 if let Some(fresh_scan) = self.scan_system_zig() {
//                     // Check the fresh scan for the target version
//                     for zv in &fresh_scan {
//                         if let ZigVersion::System {
//                             path: Some(p),
//                             version: Some(v),
//                         } = zv
//                         {
//                             if v == target_version && !self.is_zv_managed_path(p) {
//                                 // Found in fresh scan, validate and update config
//                                 self.validate_system_path_and_version(p, Some(target_version))?;

//                                 // Update config with fresh scan as side effect
//                                 if let Ok(config) = self.config_mut() {
//                                     if let Err(e) =
//                                         config.resync_system_detected(fresh_scan.clone())
//                                     {
//                                         tracing::warn!(
//                                             "Failed to update config with fresh system scan: {}",
//                                             e
//                                         );
//                                     }
//                                 }

//                                 return Ok(());
//                             }
//                         }
//                     }

//                     // Update config even if target version not found
//                     if let Ok(config) = self.config_mut() {
//                         if let Err(e) = config.resync_system_detected(fresh_scan) {
//                             tracing::warn!("Failed to update config with fresh system scan: {}", e);
//                         }
//                     }
//                 }

//                 // Version not found in either config or fresh scan
//                 Err(ZvError::SystemZigError(eyre!(
//                     "System Zig version {} not found in any system installation",
//                     target_version
//                 )))
//             }
//             (None, None) => Err(ZvError::SystemZigError(eyre!(
//                 "Cannot resolve without path or version"
//             ))),
//         }
//     }

//     /// Find any available system Zig installation (prioritizes highest stable version)
//     fn find_any_system_zig(&self) -> Result<ZigVersion, ZvError> {
//         // Try to find the latest stable version first
//         match self.find_latest_stable() {
//             Ok(version) => Ok(version),
//             Err(stable_err) => {
//                 tracing::error!(target: "libzv::system_zig", "Failed to find latest stable system Zig: {}", stable_err);
//                 // If no stable versions found, try to find the latest development version
//                 match self.find_latest_dev() {
//                     Ok(version) => Ok(version),
//                     Err(dev_err) => {
//                         tracing::error!(target: "libzv::system_zig", "Failed to find latest dev system Zig: {}", dev_err);
//                         // Neither stable nor development versions found
//                         Err(ZvError::SystemZigError(eyre!(
//                             "No system Zig installations found"
//                         )))
//                     }
//                 }
//             }
//         }
//     }
// }

// // Helper methods for the SystemZig implementation
// impl<G: Ask, Z: ZvConfig> App<G, Z> {
//     /// Validate path and version, ensuring they match and path is not zv-managed
//     /// Returns the actual version found at the path
//     fn validate_system_path_and_version(
//         &self,
//         path: &Path,
//         expected_version: Option<&semver::Version>,
//     ) -> Result<semver::Version, ZvError> {
//         // Validate path exists and is executable
//         self.validate_executable_path(path)?;

//         // Check if path is zv-managed
//         if self.is_zv_managed_path(path) {
//             return Err(ZvError::SystemZigError(eyre!(
//                 "Path {} is zv-managed, cannot use as system Zig",
//                 path.display()
//             )));
//         }

//         // Get actual version from executable
//         let actual_version = get_zig_version(path)?;

//         // If expected version is provided, verify it matches
//         if let Some(expected) = expected_version {
//             if actual_version != *expected {
//                 return Err(ZvError::SystemZigError(eyre!(
//                     "Version mismatch: expected {}, found {} at path {}",
//                     expected,
//                     actual_version,
//                     path.display()
//                 )));
//             }
//         }

//         Ok(actual_version)
//     }

//     /// Get system-detected versions from config
//     fn get_system_versions(&self) -> Result<Vec<ZigVersion>, ZvError> {
//         if let Some(config) = &self.config {
//             config.get_system_detected()
//         } else {
//             tracing::warn!("Config not loaded, performing fresh system scan");
//             // Scan system zig directly when config is not loaded
//             self.scan_system_zig()
//                 .ok_or_else(|| ZvError::SystemZigError(eyre!("No system Zig installations found")))
//         }
//     }

//     /// Get the installation directory name for a ZigVersion
//     /// For zv-managed versions, this returns the directory name used in <ZV_DIR>/versions/
//     /// Always uses the resolved semver, never placeholder or generic names
//     fn get_installation_dir_name(&self, version: &ZigVersion) -> Option<String> {
//         match version {
//             // For all zv-managed versions, we use the semantic version string
//             ZigVersion::Semver(v) => Some(v.to_string()),

//             // For network versions, always use the resolved semver (never placeholder names)
//             ZigVersion::Master(v) => {
//                 if v == &semver::Version::new(0, 0, 0) {
//                     tracing::warn!(
//                         "get_installation_dir_name called with unresolved Master version"
//                     );
//                     None
//                 } else {
//                     Some(v.to_string())
//                 }
//             }
//             ZigVersion::Stable(v) => {
//                 if v == &semver::Version::new(0, 0, 0) {
//                     tracing::warn!(
//                         "get_installation_dir_name called with unresolved Stable version"
//                     );
//                     None
//                 } else {
//                     Some(v.to_string())
//                 }
//             }
//             ZigVersion::Latest(v) => {
//                 if v == &semver::Version::new(0, 0, 0) {
//                     tracing::warn!(
//                         "get_installation_dir_name called with unresolved Latest version"
//                     );
//                     None
//                 } else {
//                     Some(v.to_string())
//                 }
//             }
//             // System versions are not stored in zv-managed directories
//             ZigVersion::System { .. } => None,
//             ZigVersion::Unknown => None,
//         }
//     }

//     /// Validate that a path exists and is executable
//     fn validate_executable_path(&self, path: &Path) -> Result<(), ZvError> {
//         if !path.exists() {
//             return Err(ZvError::SystemZigError(eyre!(
//                 "Path does not exist: {}",
//                 path.display()
//             )));
//         }

//         // Check if it's executable (Unix-specific check)
//         #[cfg(unix)]
//         {
//             let metadata = std::fs::metadata(path).map_err(ZvError::Io)?;
//             let permissions = metadata.permissions();
//             if !permissions.mode() & 0o111 != 0 {
//                 return Err(ZvError::SystemZigError(eyre!(
//                     "Path is not executable: {}",
//                     path.display()
//                 )));
//             }
//         }

//         Ok(())
//     }

//     /// Search for a system Zig version matching the target version
//     fn search_matching_version(
//         &self,
//         target_version: &semver::Version,
//     ) -> Result<ZigVersion, ZvError> {
//         let system_versions = self.get_system_versions()?;

//         for zv in system_versions {
//             if let ZigVersion::System { path, version } = zv {
//                 if let (Some(p), Some(v)) = (path, version) {
//                     if v == *target_version && !self.is_zv_managed_path(&p) {
//                         return Ok(ZigVersion::System {
//                             path: Some(p),
//                             version: Some(v),
//                         });
//                     }
//                 }
//             }
//         }

//         Err(ZvError::SystemZigError(eyre!(
//             "No system Zig version matching {} found",
//             target_version
//         )))
//     }
// }
