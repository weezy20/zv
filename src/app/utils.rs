use crate::tools::canonicalize;
use same_file::Handle;
use std::path::{Path, PathBuf};

/// Enum representing the type of shim to detect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shim {
    Zig,
    Zls,
}

impl Shim {
    /// Returns the executable name for this shim
    fn executable_name(&self) -> &'static str {
        match self {
            Shim::Zig => {
                if cfg!(target_os = "windows") {
                    "zig.exe"
                } else {
                    "zig"
                }
            }
            Shim::Zls => {
                if cfg!(target_os = "windows") {
                    "zls.exe"
                } else {
                    "zls"
                }
            }
        }
    }
}

/// Checks if a file is a valid zv shim by comparing it with the current executable
fn is_zv_shim(shim_path: &Path, current_exe_handle: &Handle) -> bool {
    // First check for hard links using same-file crate
    if let Ok(shim_handle) = Handle::from_path(shim_path) {
        if shim_handle == *current_exe_handle {
            tracing::debug!("Found ZV shim (hard link) at {:?}", shim_path);
            return true;
        }
    }

    // Check for symlinks
    if shim_path.is_symlink() {
        if let Ok(target) = std::fs::read_link(shim_path) {
            // Handle both absolute and relative symlink targets
            let resolved_target = if target.is_absolute() {
                canonicalize(&target)
            } else {
                // For relative symlinks, resolve relative to the symlink's parent directory
                if let Some(parent) = shim_path.parent() {
                    canonicalize(parent.join(&target))
                } else {
                    canonicalize(&target)
                }
            };

            if let Ok(resolved_target) = resolved_target {
                // Compare the resolved target with current exe using same-file
                if let Ok(target_handle) = Handle::from_path(&resolved_target) {
                    if target_handle == *current_exe_handle {
                        tracing::debug!("Found ZV shim (symlink) at {:?}", shim_path);
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Detect and validate ZV shim in the bin directory
/// Returns the canonicalized path if a valid ZV shim is found
pub fn detect_shim(bin_path: &Path, shim: Shim) -> Option<PathBuf> {
    let shim_file = bin_path.join(shim.executable_name());

    // Basic existence and file type check
    if !shim_file.exists() || !shim_file.is_file() {
        return None;
    }

    // Get current executable handle for comparison
    let current_exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("Failed to get current executable path: {}", e);
            return None;
        }
    };

    let current_exe_handle = match Handle::from_path(&current_exe_path) {
        Ok(handle) => handle,
        Err(e) => {
            tracing::warn!("Failed to create handle for current executable: {}", e);
            return None;
        }
    };

    #[cfg(unix)]
    {
        // On Unix, also check if the file is executable
        if let Ok(metadata) = std::fs::metadata(&shim_file) {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                tracing::debug!(
                    "File {} exists but is not executable",
                    shim.executable_name()
                );
                return None;
            }
        }
    }

    // Check if this is actually a zv shim
    if is_zv_shim(&shim_file, &current_exe_handle) {
        canonicalize(&shim_file).ok()
    } else {
        tracing::debug!(
            "File {} exists but is not a zv shim at {:?}",
            shim.executable_name(),
            shim_file
        );
        None
    }
}
