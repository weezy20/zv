use std::path::{Path, PathBuf};

/// Windows-specific function to check if two paths refer to the same file using metadata
/// This reliably detects hard links by comparing file_index and volume_serial_number
#[cfg(target_os = "windows")]
pub fn same_file_win(a: &Path, b: &Path) -> bool {
    // Unfortunately, file_index() and volume_serial_number() are unstable features
    // So we'll use the canonical path comparison and basic metadata for now
    if let (Ok(canon_a), Ok(canon_b)) = (a.canonicalize(), b.canonicalize()) {
        if canon_a == canon_b {
            return true;
        }
    }
    
    // Fallback: compare basic metadata (size, times) for hard link detection
    if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
        ma.len() == mb.len()
            && ma.created().ok() == mb.created().ok()
            && ma.modified().ok() == mb.modified().ok()
    } else {
        false
    }
}

/// Cross-platform function to check if two paths refer to the same file
/// On Unix, uses inode comparison. On Windows, uses file_index + volume_serial_number
pub fn is_same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            ma.dev() == mb.dev() && ma.ino() == mb.ino()
        } else {
            false
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        same_file_win(a, b)
    }
    
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        // Fallback for other platforms: canonicalize and compare paths
        if let (Ok(canon_a), Ok(canon_b)) = (a.canonicalize(), b.canonicalize()) {
            canon_a == canon_b
        } else {
            false
        }
    }
}

/// Detect and validate ZV zig shim in the bin directory
/// Returns the canonicalized path if a valid ZV zig shim is found
pub fn detect_zig_shim(bin_path: &Path) -> Option<PathBuf> {
    let zig_file = if cfg!(target_os = "windows") {
        bin_path.join("zig.exe")
    } else {
        bin_path.join("zig")
    };

    if !zig_file.exists() || !zig_file.is_file() {
        return None;
    }

    #[cfg(unix)]
    {
        // On Unix, check if it's executable and if it's our zv binary (hard link/symlink detection)
        if let Ok(metadata) = std::fs::metadata(&zig_file) {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                if let Ok(current_exe) = std::env::current_exe() {
                    if is_same_file(&zig_file, &current_exe) {
                        tracing::debug!("Found ZV zig shim (hard link or same inode) at {:?}", zig_file);
                        return zig_file.canonicalize().ok();
                    }
                    // Check if it's a symlink pointing to our binary
                    else if zig_file.is_symlink() {
                        if let Ok(target) = std::fs::read_link(&zig_file) {
                            if let Ok(resolved_target) = target.canonicalize() {
                                if resolved_target == current_exe {
                                    tracing::debug!("Found ZV zig shim (symlink) at {:?}", zig_file);
                                    return zig_file.canonicalize().ok();
                                }
                            }
                        }
                    }
                    // If we still haven't found it, but we know it exists and is executable, assume it's our shim
                    else {
                        tracing::debug!("Found executable zig in ZV bin dir, assuming it's our shim: {:?}", zig_file);
                        return zig_file.canonicalize().ok();
                    }
                } else {
                    // Fallback: assume it's our shim if it's executable
                    return zig_file.canonicalize().ok();
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, check if zig.exe is actually our zv.exe (hard link detection)
        if let Ok(current_exe) = std::env::current_exe() {
            if same_file_win(&zig_file, &current_exe) {
                tracing::debug!("Found ZV zig shim (hard link or same file) at {:?}", zig_file);
                return zig_file.canonicalize().ok();
            }
            // Check if it's a symlink pointing to our binary
            else if zig_file.is_symlink() {
                if let Ok(target) = std::fs::read_link(&zig_file) {
                    if let Ok(resolved_target) = target.canonicalize() {
                        if resolved_target == current_exe {
                            tracing::debug!("Found ZV zig shim (symlink) at {:?}", zig_file);
                            return zig_file.canonicalize().ok();
                        }
                    }
                }
            }
            // If we still haven't found it, but we know it exists in our bin dir, assume it's our shim
            else {
                tracing::debug!("Found zig executable in ZV bin dir, assuming it's our shim: {:?}", zig_file);
                return zig_file.canonicalize().ok();
            }
        } else {
            // Fallback: assume .exe is our shim if it exists and is a file
            return zig_file.canonicalize().ok();
        }
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        // For other platforms, assume executable if file exists
        return zig_file.canonicalize().ok();
    }

    None
}
