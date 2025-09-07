// src/shell/path_utils.rs
use super::Shell;
use crate::{
    app::App,
    tools::{canonicalize, warn},
};
use std::path::Path;

/// Helper method to determine path string formatting based on shell and environment
/// Returns a tuple of (zv_dir_str, zv_bin_path_str)
pub fn get_path_strings(shell: &Shell, app: &App, using_env_var: bool) -> (String, String) {
    let zv_dir = app.path();
    let bin_path = app.bin_path();

    if using_env_var {
        // Using ZV_DIR env var, use absolute paths
        format_absolute_paths(shell, zv_dir, bin_path)
    } else {
        // Using default path, use ${HOME}/.zv with validation
        get_default_path_strings(shell)
    }
}

/// Format absolute paths, handling Windows path conversion for Unix-like shells
pub fn format_absolute_paths(shell: &Shell, zv_dir: &Path, bin_path: &Path) -> (String, String) {
    // Use the normalize_path_for_shell utility for consistent path formatting
    (
        normalize_path_for_shell(shell, zv_dir),
        normalize_path_for_shell(shell, bin_path),
    )
}

/// Get default path strings using environment variables with validation
pub fn get_default_path_strings(shell: &Shell) -> (String, String) {
    match shell {
        Shell::PowerShell => {
            // PowerShell on Unix should use Unix-style paths
            if shell.is_powershell_in_unix() {
                // Unix-like shells - check if HOME is set (warn on Unix-like systems or Unix shells on Windows)
                if std::env::var("HOME").is_ok() {
                    ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
                } else {
                    warn(
                        "HOME environment variable is not set. PowerShell on Unix requires HOME to be set for zv to work properly.",
                    );
                    ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
                }
            } else {
                // Windows PowerShell
                // Check if HOME is set for PowerShell (warn on Windows or Unix shells on Windows)
                if std::env::var("HOME").is_ok() {
                    (
                        "$env:HOME\\.zv".to_string(),
                        "$env:HOME\\.zv\\bin".to_string(),
                    )
                } else {
                    if cfg!(windows) || shell.is_unix_shell_in_windows() {
                        warn(
                            "HOME environment variable is not set. PowerShell requires HOME to be set for zv to work properly.",
                        );
                    }
                    (
                        "$env:HOME\\.zv".to_string(),
                        "$env:HOME\\.zv\\bin".to_string(),
                    )
                }
            }
        }
        Shell::Cmd => {
            // Check if USERPROFILE is set for CMD (warn on Windows, but not for Unix shells on Windows)
            if std::env::var("USERPROFILE").is_ok() {
                (
                    "%USERPROFILE%\\.zv".to_string(),
                    "%USERPROFILE%\\.zv\\bin".to_string(),
                )
            } else {
                if cfg!(windows) && !shell.is_unix_shell_in_windows() {
                    warn(
                        "USERPROFILE environment variable is not set. This is unusual on Windows systems.",
                    );
                }
                (
                    "%USERPROFILE%\\.zv".to_string(),
                    "%USERPROFILE%\\.zv\\bin".to_string(),
                )
            }
        }
        _ => {
            // Unix-like shells - check if HOME is set (warn on Unix-like systems or Unix shells on Windows)
            if std::env::var("HOME").is_ok() {
                ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
            } else {
                if cfg!(unix) || shell.is_unix_shell_in_windows() {
                    warn(
                        "HOME environment variable is not set. Unix-like shells require HOME to be set for zv to work properly.",
                    );
                }
                ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
            }
        }
    }
}

/// Check if path/to/dir is in system PATH
pub fn check_dir_in_path(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    // Canonicalize the target path once
    let target_path = match canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };

    // Get PATH environment variable
    let path_var = match std::env::var("PATH") {
        Ok(var) => var,
        Err(_) => return false,
    };

    // Use the platform-appropriate separator
    // Note: This is a generic function, so we use OS-level detection
    // For shell-specific behavior, use check_dir_in_path_for_shell
    let separator = if cfg!(windows) { ';' } else { ':' };

    path_var
        .split(separator)
        .filter(|p| !p.is_empty()) // Skip empty entries
        .map(Path::new)
        .filter(|p| p.is_dir()) // Only consider existing directories
        .filter_map(|p| canonicalize(p).ok()) // Only consider paths we can canonicalize
        .any(|candidate_path| candidate_path == target_path)
}

/// Check if path/to/dir is in system PATH, shell-aware version
pub fn check_dir_in_path_for_shell(shell: &Shell, path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }

    // Canonicalize the target path once
    let target_path = match canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };

    // Get PATH environment variable
    let path_var = match std::env::var("PATH") {
        Ok(var) => var,
        Err(_) => return false,
    };

    // Use shell-specific separator
    let separator = shell.get_path_separator();

    path_var
        .split(separator)
        .filter(|p| !p.is_empty()) // Skip empty entries
        .map(Path::new)
        .filter(|p| p.is_dir()) // Only consider existing directories
        .filter_map(|p| canonicalize(p).ok()) // Only consider paths we can canonicalize
        .any(|candidate_path| candidate_path == target_path)
}

/// Normalize path separators for the target shell environment
pub fn normalize_path_for_shell(shell: &Shell, path: &Path) -> String {
    let path_str = path.to_string_lossy();

    if shell.is_windows_shell() && !shell.is_powershell_in_unix() {
        // Windows shells expect backslashes - always convert forward slashes to backslashes
        path_str.replace('/', "\\")
    } else if shell.is_unix_shell_in_windows() {
        // Unix shells on Windows (WSL, GitBash, MinGW, etc.) need Unix-style forward slashes
        path_str.replace('\\', "/")
    } else {
        // Unix-like shells expect forward slashes - always convert backslashes to forward slashes
        // This includes PowerShell on Unix
        path_str.replace('\\', "/")
    }
}

/// Escape path for shell-specific quoting rules
pub fn escape_path_for_shell(shell: &Shell, path: &str) -> String {
    match shell {
        Shell::PowerShell => {
            // PowerShell on Unix should use Unix-style escaping
            if shell.is_powershell_in_unix() {
                // POSIX-compatible escaping for PowerShell on Unix
                if path.contains(' ') || path.contains('$') || path.contains('\\') || path.contains('`')
                {
                    format!("'{}'", path.replace('\'', "'\"'\"'"))
                } else {
                    path.to_string()
                }
            } else {
                // Windows PowerShell uses single quotes for literal strings or escapes special chars
                if path.contains(' ') || path.contains('$') || path.contains('`') {
                    format!("'{}'", path.replace('\'', "''"))
                } else {
                    path.to_string()
                }
            }
        }
        Shell::Cmd => {
            // CMD uses double quotes and doesn't need much escaping
            if path.contains(' ') {
                format!("\"{}\"", path)
            } else {
                path.to_string()
            }
        }
        Shell::Fish => {
            // Fish shell quoting
            if path.contains(' ') || path.contains('$') || path.contains('\\') {
                format!("'{}'", path.replace('\'', "\\'"))
            } else {
                path.to_string()
            }
        }
        _ => {
            // POSIX-compatible shells (bash, zsh, etc.)
            if path.contains(' ') || path.contains('$') || path.contains('\\') || path.contains('`')
            {
                format!("'{}'", path.replace('\'', "'\"'\"'"))
            } else {
                path.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_get_path_separator() {
        // PowerShell separator depends on operating system
        if cfg!(unix) {
            assert_eq!(Shell::PowerShell.get_path_separator(), ':'); // PowerShell on Unix uses colon
        } else {
            assert_eq!(Shell::PowerShell.get_path_separator(), ';'); // PowerShell on Windows uses semicolon
        }
        assert_eq!(Shell::Cmd.get_path_separator(), ';');
        assert_eq!(Shell::Bash.get_path_separator(), ':');
        assert_eq!(Shell::Fish.get_path_separator(), ':');
        assert_eq!(Shell::Zsh.get_path_separator(), ':');
    }

    #[test]
    fn test_path_separator_unix_shell_in_windows_aware() {
        // Unix shells should use colon even when running on Windows (WSL, GitBash, etc.)
        assert_eq!(Shell::Bash.get_path_separator(), ':');
        assert_eq!(Shell::Zsh.get_path_separator(), ':');
        assert_eq!(Shell::Fish.get_path_separator(), ':');
        
        // Windows shells should use semicolon (except PowerShell on Unix)
        if cfg!(windows) {
            assert_eq!(Shell::PowerShell.get_path_separator(), ';');
        }
        assert_eq!(Shell::Cmd.get_path_separator(), ';');
    }

    #[test]
    fn test_escape_path_for_shell() {
        let path_with_spaces = "/path with spaces/bin";

        assert_eq!(
            escape_path_for_shell(&Shell::Bash, path_with_spaces),
            "'/path with spaces/bin'"
        );

        assert_eq!(
            escape_path_for_shell(&Shell::Cmd, path_with_spaces),
            "\"/path with spaces/bin\""
        );

        let simple_path = "/simple/path";
        assert_eq!(
            escape_path_for_shell(&Shell::Bash, simple_path),
            "/simple/path"
        );
    }

    #[test]
    fn test_normalize_path_for_shell() {
        let unix_path = PathBuf::from("/home/user/.zv/bin");
        let windows_path = PathBuf::from("C:\\Users\\user\\.zv\\bin");

        // Unix shell should use forward slashes
        assert!(normalize_path_for_shell(&Shell::Bash, &windows_path).contains('/'));

        // Windows shell should use backslashes
        assert!(normalize_path_for_shell(&Shell::Cmd, &unix_path).contains('\\'));
    }

    #[test]
    fn test_normalize_path_unix_shell_in_windows_aware() {
        let mixed_path = PathBuf::from("C:\\Users\\user\\mixed/path\\example");
        
        // Unix shells (including on Windows like WSL, GitBash, MinGW) should normalize to forward slashes
        let bash_result = normalize_path_for_shell(&Shell::Bash, &mixed_path);
        assert!(!bash_result.contains('\\'));
        assert!(bash_result.contains('/'));
        
        let zsh_result = normalize_path_for_shell(&Shell::Zsh, &mixed_path);
        assert!(!zsh_result.contains('\\'));
        assert!(zsh_result.contains('/'));
        
        // Windows shells should normalize to backslashes (except PowerShell on Unix)
        let cmd_result = normalize_path_for_shell(&Shell::Cmd, &mixed_path);
        assert!(cmd_result.contains('\\'));
        assert!(!cmd_result.contains('/'));
        
        // PowerShell behavior depends on operating system
        let ps_result = normalize_path_for_shell(&Shell::PowerShell, &mixed_path);
        if cfg!(unix) {
            // PowerShell on Unix should use forward slashes
            assert!(ps_result.contains('/'));
            assert!(!ps_result.contains('\\'));
        } else {
            // PowerShell on Windows should use backslashes
            assert!(ps_result.contains('\\'));
            assert!(!ps_result.contains('/'));
        }
    }

    #[test]
    fn test_get_default_path_strings() {
        let (zv_dir, zv_bin) = get_default_path_strings(&Shell::PowerShell);
        
        // PowerShell behavior depends on the target OS
        if cfg!(unix) {
            // PowerShell on Unix should use Unix-style paths
            assert!(zv_dir.contains("${HOME}"));
            assert!(zv_bin.contains("${HOME}"));
        } else {
            // PowerShell on Windows should use PowerShell-style paths
            assert!(zv_dir.contains("$env:HOME"));
            assert!(zv_bin.contains("$env:HOME"));
        }

        let (zv_dir, zv_bin) = get_default_path_strings(&Shell::Cmd);
        assert!(zv_dir.contains("%USERPROFILE%"));
        assert!(zv_bin.contains("%USERPROFILE%"));

        let (zv_dir, zv_bin) = get_default_path_strings(&Shell::Bash);
        assert!(zv_dir.contains("${HOME}"));
        assert!(zv_bin.contains("${HOME}"));
    }

    #[test]
    fn test_format_absolute_paths_utilizes_normalize() {
        let unix_path = PathBuf::from("/home/user/.zv");
        let unix_bin_path = PathBuf::from("/home/user/.zv/bin");
        
        let (zv_dir, bin_path) = format_absolute_paths(&Shell::Bash, &unix_path, &unix_bin_path);
        
        // Should use forward slashes for Unix shells
        assert!(zv_dir.contains('/'));
        assert!(bin_path.contains('/'));
        
        let (zv_dir, bin_path) = format_absolute_paths(&Shell::Cmd, &unix_path, &unix_bin_path);
        
        // Should use backslashes for Windows shells
        assert!(zv_dir.contains('\\'));
        assert!(bin_path.contains('\\'));
    }

    #[test]
    fn test_check_dir_in_path_for_shell_vs_generic() {
        // Both functions should behave the same for valid paths
        // but the shell-aware version uses the correct separator
        let test_path = PathBuf::from("/nonexistent/path");
        
        let generic_result = check_dir_in_path(&test_path);
        let shell_aware_result = check_dir_in_path_for_shell(&Shell::Bash, &test_path);
        
        // Both should return false for nonexistent paths
        assert_eq!(generic_result, shell_aware_result);
    }

    #[test] 
    fn test_powershell_on_unix_behavior() {
        if cfg!(unix) {
            let shell = Shell::PowerShell;
            
            // PowerShell on Unix should behave like Unix shells for paths
            assert_eq!(shell.get_path_separator(), ':');
            
            let unix_path = PathBuf::from("/home/user/.zv");
            let normalized = normalize_path_for_shell(&shell, &unix_path);
            assert!(normalized.contains('/'));
            assert!(!normalized.contains('\\'));
            
            // Environment file should be Unix-style
            assert_eq!(shell.env_file_name(), "env");
            
            // Should get Unix-style RC files
            let rc_files = shell.get_rc_files();
            assert!(!rc_files.is_empty()); // Should have .profile
            
            // Path strings should use Unix-style variables
            let (zv_dir, _) = get_default_path_strings(&shell);
            assert!(zv_dir.contains("${HOME}"));
            assert!(zv_dir.contains(".zv"));
            assert!(!zv_dir.contains("$env:HOME"));
        }
    }
}
