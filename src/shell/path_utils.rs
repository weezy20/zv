// src/shell/path_utils.rs
use super::Shell;
use crate::{app::App, tools::canonicalize};
use std::path::Path;

/// Helper method to determine path string formatting based on shell and environment
pub fn get_path_strings(shell: &Shell, app: &App, using_env_var: bool) -> (String, String) {
    let zv_dir = app.path();
    let bin_path = app.bin_path();

    if using_env_var {
        // Using ZV_DIR env var, use absolute paths
        format_absolute_paths(shell, zv_dir, bin_path)
    } else {
        // Using default path, use ${HOME}/.zv
        get_default_path_strings(shell)
    }
}

/// Format absolute paths, handling Windows path conversion for Unix-like shells
pub fn format_absolute_paths(shell: &Shell, zv_dir: &Path, bin_path: &Path) -> (String, String) {
    if cfg!(windows) && shell.is_unix_shell() {
        // Convert both paths consistently for Unix-like shells on Windows
        (
            zv_dir.to_string_lossy().replace('\\', "/"),
            bin_path.to_string_lossy().replace('\\', "/"),
        )
    } else {
        (
            zv_dir.to_string_lossy().into_owned(),
            bin_path.to_string_lossy().into_owned(),
        )
    }
}

/// Get default path strings using environment variables
pub fn get_default_path_strings(shell: &Shell) -> (String, String) {
    match shell {
        Shell::PowerShell => (
            "$env:HOME\\.zv".to_string(),
            "$env:HOME\\.zv\\bin".to_string(),
        ),
        Shell::Cmd => (
            "%USERPROFILE%\\.zv".to_string(),
            "%USERPROFILE%\\.zv\\bin".to_string(),
        ),
        _ => {
            // Unix-like shells
            ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
        }
    }
}

/// Check if path is in system PATH
pub fn check_path_in_system(path: &Path) -> bool {
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

    let separator = if cfg!(windows) { ';' } else { ':' };

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

    match shell {
        Shell::PowerShell | Shell::Cmd => {
            // Windows shells expect backslashes - always convert forward slashes to backslashes
            path_str.replace('/', "\\")
        }
        _ => {
            // Unix-like shells expect forward slashes - always convert backslashes to forward slashes
            path_str.replace('\\', "/")
        }
    }
}

/// Get the appropriate PATH separator for the shell
pub fn get_path_separator(shell: &Shell) -> char {
    match shell {
        Shell::PowerShell | Shell::Cmd => ';',
        _ => ':',
    }
}

/// Escape path for shell-specific quoting rules
pub fn escape_path_for_shell(shell: &Shell, path: &str) -> String {
    match shell {
        Shell::PowerShell => {
            // PowerShell uses single quotes for literal strings or escapes special chars
            if path.contains(' ') || path.contains('$') || path.contains('`') {
                format!("'{}'", path.replace('\'', "''"))
            } else {
                path.to_string()
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
        assert_eq!(get_path_separator(&Shell::PowerShell), ';');
        assert_eq!(get_path_separator(&Shell::Cmd), ';');
        assert_eq!(get_path_separator(&Shell::Bash), ':');
        assert_eq!(get_path_separator(&Shell::Fish), ':');
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
    fn test_get_default_path_strings() {
        let (zv_dir, zv_bin) = get_default_path_strings(&Shell::PowerShell);
        assert!(zv_dir.contains("$env:HOME"));
        assert!(zv_bin.contains("$env:HOME"));

        let (zv_dir, zv_bin) = get_default_path_strings(&Shell::Cmd);
        assert!(zv_dir.contains("%USERPROFILE%"));
        assert!(zv_bin.contains("%USERPROFILE%"));

        let (zv_dir, zv_bin) = get_default_path_strings(&Shell::Bash);
        assert!(zv_dir.contains("${HOME}"));
        assert!(zv_bin.contains("${HOME}"));
    }
}
