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
    Tcsh,
    Posix,
    Nu,
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
            } else if shell.contains("tcsh") || shell.contains("csh") {
                return Shell::Tcsh;
            } else if shell.contains("nu") {
                return Shell::Nu;
            } else if shell.contains("sh") && !shell.contains("bash") && !shell.contains("zsh") {
                return Shell::Posix;
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
            Shell::Fish => "env.fish",
            Shell::Nu => "env.nu",
            Shell::Tcsh => "env.csh",
            _ => "env", // bash, zsh, and other POSIX shells
        }
    }

    /// Returns the env file path and content without writing to disk
    pub fn export_without_dump(&self, zv_dir: &Path, using_env_var: bool) -> (PathBuf, String) {
        let env_file = zv_dir.join(self.env_file_name());
        let bin_path = zv_dir.join("bin"); // The actual bin directory to add to PATH
        
        // Use ${HOME}/.zv when using default path, otherwise use absolute path
        let (zv_dir_str, zv_bin_path_str) = if using_env_var {
            // Using ZV_DIR env var, use absolute paths
            (
                zv_dir.to_string_lossy().into_owned(),
                if cfg!(windows) && matches!(self, Shell::Bash | Shell::Zsh | Shell::Fish) {
                    // Convert Windows path separators to Unix-style for Unix-like shells on Windows (e.g., WSL)
                    bin_path.to_string_lossy().replace('\\', "/")
                } else {
                    bin_path.to_string_lossy().into_owned()
                }
            )
        } else {
            // Using default path, use ${HOME}/.zv
            ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
        };

        let env_content = match self {
            Shell::PowerShell => {
                format!(
                    r#"# zv shell setup for PowerShell
# To permanently set environment variables in PowerShell, run as Administrator:
# [Environment]::SetEnvironmentVariable("ZV_DIR", "{zv_dir}", "User")
# [Environment]::SetEnvironmentVariable("PATH", "{path};$env:PATH", "User")

$env:ZV_DIR = "{zv_dir}"
if ($env:PATH -notlike "*{path}*") {{
    $env:PATH = "{path};$env:PATH"
}}"#,
                    path = zv_bin_path_str,
                    zv_dir = zv_dir_str
                )
            }
            Shell::Cmd => {
                format!(
                    r#"REM zv shell setup for Command Prompt
REM To permanently set environment variables in CMD, run as Administrator:
REM setx ZV_DIR "{zv_dir}" /M
REM setx PATH "{path};%PATH%" /M

set "ZV_DIR={zv_dir}"
echo ;%PATH%; | find /i ";{path};" >nul || set "PATH={path};%PATH%""#,
                    path = zv_bin_path_str,
                    zv_dir = zv_dir_str
                )
            }
            Shell::Fish => {
                format!(
                    r#"#!/usr/bin/env fish
# zv shell setup for Fish shell
set -gx ZV_DIR "{zv_dir}"
if not contains "{path}" $PATH
    set -gx PATH "{path}" $PATH
end"#,
                    path = zv_bin_path_str,
                    zv_dir = zv_dir_str
                )
            }
            Shell::Nu => {
                format!(
                    r#"# zv shell setup for Nushell
$env.ZV_DIR = "{zv_dir}"
$env.PATH = ($env.PATH | split row (char esep) | prepend "{path}" | uniq)"#,
                    path = zv_bin_path_str,
                    zv_dir = zv_dir_str
                )
            }
            Shell::Tcsh => {
                format!(
                    r#"#!/bin/csh
# zv shell setup for tcsh/csh
setenv ZV_DIR "{zv_dir}"
echo ":${{PATH}}:" | grep -q ":{path}:" || setenv PATH "{path}:$PATH""#,
                    path = zv_bin_path_str,
                    zv_dir = zv_dir_str
                )
            }
            Shell::Bash | Shell::Zsh | Shell::Posix | Shell::Unknown => {
                // POSIX-compliant syntax with robust PATH checking (similar to Cargo)
                format!(
                    r#"#!/bin/sh
# zv shell setup
# affix colons on either side of $PATH to simplify matching
export ZV_DIR="{zv_dir}"
case ":${{PATH}}:" in
    *:"{path}":*)
        ;;
    *)
        # Prepending path in case a system-installed binary needs to be overridden
        export PATH="{path}:$PATH"
        ;;
esac"#,
                    path = zv_bin_path_str,
                    zv_dir = zv_dir_str
                )
            }
        };

        if matches!(self, Shell::Unknown) {
            tracing::warn!("Unknown shell type detected, using POSIX shell syntax");
        }

        (env_file, env_content)
    }
    /// Dumps shell specific environment variables to the env file, overwriting if read errors
    /// For CMD and PowerShell, this method does not write to disk as system variables are edited directly
    pub async fn export(&self, zv_dir: &Path, using_env_var: bool) -> Result<(), ZvError> {
        if matches!(self, Shell::Cmd | Shell::PowerShell) {
            return Ok(());
        }

        let (env_file, content) = self.export_without_dump(zv_dir, using_env_var);

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
            Shell::Tcsh => write!(f, "tcsh"),
            Shell::Posix => write!(f, "posix"),
            Shell::Nu => write!(f, "nu"),
            Shell::Unknown => write!(f, "unknown"),
        }
    }
}
