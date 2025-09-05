use color_eyre::eyre::eyre;
use std::path::{Path, PathBuf};
use sysinfo::{Pid, Process, System};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};
use crate::tools::is_tty;
use crate::ZvError;

/// Get the parent process name using sysinfo
fn get_parent_process_name() -> Option<String> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::everything(),
    );

    let current_pid = sysinfo::get_current_pid().ok()?;
    let current_process = system.process(current_pid)?;
    let parent_pid = current_process.parent()?;
    let parent_process = system.process(parent_pid)?;

    Some(parent_process.name().to_string_lossy().to_string())
}

/// Detect shell from parent process name
fn detect_shell_from_parent() -> Option<Shell> {
    let parent_name = get_parent_process_name()?;
    let parent_lower = parent_name.to_lowercase();

    if parent_lower.contains("bash") {
        Some(Shell::Bash)
    } else if parent_lower.contains("zsh") {
        Some(Shell::Zsh)
    } else if parent_lower.contains("fish") {
        Some(Shell::Fish)
    } else if parent_lower.contains("powershell") || parent_lower.contains("pwsh") {
        Some(Shell::PowerShell)
    } else if parent_lower.contains("cmd") {
        Some(Shell::Cmd)
    } else if parent_lower.contains("tcsh") || parent_lower.contains("csh") {
        Some(Shell::Tcsh)
    } else if parent_lower.contains("nu") {
        Some(Shell::Nu)
    } else {
        None
    }
}

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
    /// Detect shell from environment with improved reliability
    pub fn detect() -> Shell {
        // Closure to detect shell from any string containing shell information
        let detect_shell_from_string = |shell_str: &str| -> Option<Shell> {
            if shell_str.contains("bash") {
                Some(Shell::Bash)
            } else if shell_str.contains("zsh") {
                Some(Shell::Zsh)
            } else if shell_str.contains("fish") {
                Some(Shell::Fish)
            } else if shell_str.contains("powershell") || shell_str.contains("pwsh") {
                Some(Shell::PowerShell)
            } else if shell_str.contains("cmd") {
                Some(Shell::Cmd)
            } else if shell_str.contains("tcsh") || shell_str.contains("csh") {
                Some(Shell::Tcsh)
            } else if shell_str.contains("nu") {
                Some(Shell::Nu)
            } else if shell_str.contains("sh") && !shell_str.contains("bash") && !shell_str.contains("zsh") {
                Some(Shell::Posix)
            } else {
                None
            }
        };

        if cfg!(windows) {
            // Windows-specific detection
            Self::detect_windows_shell(detect_shell_from_string)
        } else {
            // Unix-like systems detection
            Self::detect_unix_shell(detect_shell_from_string)
        }
    }

    /// Windows-specific shell detection
    fn detect_windows_shell<F>(detect_shell_from_string: F) -> Shell 
    where 
        F: Fn(&str) -> Option<Shell>
    {
        // First, try to detect from parent process if we're in a TTY
        if is_tty() {
            if let Some(shell) = detect_shell_from_parent() {
                return shell;
            }
        }

        // Check if we're in WSL (Unix shells on Windows)
        if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
            // In WSL, SHELL variable should work properly
            if let Ok(shell) = std::env::var("SHELL") {
                if let Some(detected) = detect_shell_from_string(&shell) {
                    return detected;
                }
            }
            return Shell::Bash; // Default for WSL
        }

        // Check for PowerShell environment indicators
        if std::env::var("PSModulePath").is_ok() {
            return Shell::PowerShell;
        }

        // Additional checks for specific environments
        if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
            if term_program == "vscode" {
                // VS Code integrated terminal, check for shell preference
                if let Ok(vscode_shell) = std::env::var("VSCODE_SHELL_INTEGRATION") {
                    if let Some(shell) = detect_shell_from_string(&vscode_shell) {
                        return shell;
                    }
                }
            }
        }

        // Default to PowerShell on modern Windows
        Shell::PowerShell
    }

    /// Unix-like systems shell detection
    fn detect_unix_shell<F>(detect_shell_from_string: F) -> Shell 
    where 
        F: Fn(&str) -> Option<Shell>
    {
        // First, try to detect from parent process if we're in a TTY
        if is_tty() {
            if let Some(shell) = detect_shell_from_parent() {
                return shell;
            }
        }

        // Use SHELL environment variable (standard on Unix-like systems)
        if let Ok(shell) = std::env::var("SHELL") {
            if let Some(detected) = detect_shell_from_string(&shell) {
                return detected;
            }
        }

        // Additional checks for specific environments
        if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
            if term_program == "vscode" {
                // VS Code integrated terminal, check for shell preference
                if let Ok(vscode_shell) = std::env::var("VSCODE_SHELL_INTEGRATION") {
                    if let Some(shell) = detect_shell_from_string(&vscode_shell) {
                        return shell;
                    }
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

    /// Get shell RC files that should be modified for this shell type
    pub fn get_rc_files(&self) -> Vec<PathBuf> {
        let home_dir = match dirs::home_dir() {
            Some(dir) => dir,
            None => return vec![],
        };

        match self {
            Shell::Bash => vec![home_dir.join(".profile"), home_dir.join(".bashrc")],
            Shell::Zsh => vec![home_dir.join(".profile"), home_dir.join(".zshrc")],
            Shell::Fish => vec![home_dir.join(".config/fish/config.fish")],
            Shell::Tcsh => vec![home_dir.join(".profile"), home_dir.join(".tcshrc")],
            Shell::Nu => vec![home_dir.join(".config/nushell/config.nu")],
            Shell::Posix | Shell::Unknown => vec![home_dir.join(".profile")],
            Shell::PowerShell | Shell::Cmd => vec![], // Windows doesn't use RC files
        }
    }

    /// Generate the source command for this shell type
    pub fn get_source_command(&self, env_file: &Path) -> String {
        match self {
            Shell::Fish => format!("source \"{}\"", env_file.display()),
            Shell::Nu => format!("source \"{}\"", env_file.display()),
            Shell::Tcsh => format!("source \"{}\"", env_file.display()),
            _ => format!("source \"{}\"", env_file.display()), // POSIX shells (bash, zsh, etc.)
        }
    }

    /// Returns the env file path and content without writing to disk
    pub fn export_without_dump(
        &self,
        zv_dir: &Path,
        bin_path: &Path,
        using_env_var: bool,
    ) -> (PathBuf, String) {
        let env_file = zv_dir.join(self.env_file_name());

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
                },
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
    pub async fn export(
        &self,
        zv_dir: &Path,
        bin_path: &Path,
        using_env_var: bool,
    ) -> Result<(), ZvError> {
        if matches!(self, Shell::Cmd | Shell::PowerShell) {
            return Ok(());
        }

        let (env_file, content) = self.export_without_dump(zv_dir, bin_path, using_env_var);

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
