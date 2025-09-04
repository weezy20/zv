use color_eyre::eyre::eyre;
use std::path::{Path, PathBuf};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use crate::ZvError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Cmd,
    Unknown,
}

impl Shell {
    /// Detect shell from environment
    pub fn detect() -> Shell {
        if let Ok(shell) = std::env::var("SHELL") {
            if shell.contains("bash") {
                return Shell::Bash;
            } else if shell.contains("zsh") {
                return Shell::Zsh;
            } else if shell.contains("fish") {
                return Shell::Fish;
            }
        }

        // Windows shell (powershell/cmd) detection
        if cfg!(windows) {
            if std::env::var("PSModulePath").is_ok() {
                return Shell::PowerShell;
            } else if let Ok(comspec) = std::env::var("COMSPEC") {
                if comspec.to_lowercase().contains("cmd") {
                    return Shell::Cmd;
                }
            }
        }

        Shell::Unknown
    }

    /// Get the appropriate env file name based on shell type
    pub fn env_file_name(&self) -> &'static str {
        match self {
            Shell::PowerShell => "env.ps1",
            Shell::Cmd => "env.bat",
            _ => "env", // bash, zsh, fish or other *nix shells incl. Shell::Unknown
        }
    }

    /// Returns the env file path and content without writing to disk
    pub fn export_without_dump(&self, zv_dir: &Path) -> (PathBuf, String) {
        let env_file = zv_dir.join(self.env_file_name()); // app.env_path
        let zv_bin_path_str =
            if cfg!(windows) && matches!(self, Shell::Bash | Shell::Zsh | Shell::Fish) {
                // Convert Windows path separators to Unix-style for Unix-like shells on Windows (e.g., WSL)
                env_file.to_string_lossy().replace('\\', "/")
            } else {
                env_file.to_string_lossy().into_owned()
            };

        let env_content = match self {
            Shell::PowerShell => {
                // No need for extra escaping; PowerShell handles paths well
                format!(r#"$env:PATH = "{path};$env:PATH""#, path = zv_bin_path_str)
            }
            Shell::Cmd => {
                // Double quotes to handle spaces
                format!(r#"set "PATH={path};%PATH%""#, path = zv_bin_path_str)
            }
            Shell::Fish => {
                // Fish-specific syntax for setting PATH
                format!(r#"set -gx PATH "{path}" $PATH"#, path = zv_bin_path_str)
            }
            Shell::Bash | Shell::Zsh => {
                // Double quotes to handle special characters
                format!(r#"export PATH="{path}:$PATH""#, path = zv_bin_path_str)
            }
            Shell::Unknown => {
                tracing::warn!("Unknown shell type detected, using bash shell syntax");
                // Conservative default
                format!(r#"export PATH="{path}:$PATH""#, path = zv_bin_path_str)
            }
        };

        (env_file, env_content)
    }
    /// Dumps shell specific environment variables to the env file, overwriting if read errors
    pub async fn export(&self, zv_dir: &Path) -> Result<(), ZvError> {
        let (env_file, content) = self.export_without_dump(zv_dir);

        // Check if content already exists in file
        let dump_true = if env_file.exists() {
            let existing_content = tokio::fs::read_to_string(&env_file)
                .await
                .ok()
                .unwrap_or_default();
            !existing_content.contains(&content)
        } else {
            true
        };

        if dump_true {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&env_file)
                .await
                .map_err(|e: std::io::Error| {
                    ZvError::ZvExportError(eyre!(e).wrap_err("Failed to open env file for writing"))
                })?;

            AsyncWriteExt::write_all(&mut file, content.as_bytes())
                .await
                .map_err(|e: std::io::Error| {
                    ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write to env file"))
                })?;
            AsyncWriteExt::write_all(&mut file, b"\n")
                .await
                .map_err(|e: std::io::Error| {
                    ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write newline to env file"))
                })?;
        }
        Ok(())
    }

    /// Based on current shell type, inspect `path` is in SHELL's PATH
    pub fn check_path_in_system(&self, path: &Path) -> bool {
        if !path.is_dir() {
            return false;
        }

        // Canonicalize the target path once
        let target_path = match path.canonicalize() {
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
            .filter_map(|p| p.canonicalize().ok()) // Only consider paths we can canonicalize
            .any(|candidate_path| candidate_path == target_path)
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::detect()
    }
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shell::Bash => write!(f, "bash"),
            Shell::Zsh => write!(f, "zsh"),
            Shell::PowerShell => write!(f, "powershell"),
            Shell::Fish => write!(f, "fish"),
            Shell::Cmd => write!(f, "cmd"),
            Shell::Unknown => write!(f, "unknown"),
        }
    }
}
