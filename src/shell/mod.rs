use crate::ZvError;
use crate::app::App;
use crate::tools::{canonicalize, is_tty};
use color_eyre::eyre::eyre;
use std::path::{Path, PathBuf};
use sysinfo::{Pid, Process, System};
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

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
            } else if shell_str.contains("sh")
                && !shell_str.contains("bash")
                && !shell_str.contains("zsh")
            {
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
        F: Fn(&str) -> Option<Shell>,
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
        F: Fn(&str) -> Option<Shell>,
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
    /// Is non-windows shell?
    #[inline]
    pub fn is_unix_shell(&self) -> bool {
        !matches!(self, Shell::Cmd | Shell::PowerShell)
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
            Shell::Bash => vec![
                home_dir.join(".bashrc"),
                home_dir.join(".profile"),
                home_dir.join(".bash_profile"),
            ],
            Shell::Zsh => vec![
                home_dir.join(".zshenv"),
                home_dir.join(".zshrc"),
                home_dir.join(".zprofile"),
            ],
            Shell::Fish => vec![home_dir.join(".config/fish/config.fish")],
            Shell::Tcsh => vec![home_dir.join(".profile"), home_dir.join(".tcshrc")],
            Shell::Nu => vec![home_dir.join(".config/nushell/config.nu")],
            Shell::Posix | Shell::Unknown => vec![home_dir.join(".profile")],
            Shell::PowerShell | Shell::Cmd => vec![], // Windows doesn't use RC files
        }
    }

    /// Generate the source command for this shell type
    pub fn get_source_command(&self, env_file: &Path) -> String {
        format!("source \"{}\"", env_file.display())
    }

    /// Returns the env file path and content without writing to disk
    pub fn export_without_dump<'a>(&self, app: &'a App, using_env_var: bool) -> (&'a Path, String) {
        let env_file = app.env_path().as_path();
        let (zv_dir_str, zv_bin_path_str) = self.get_path_strings(app, using_env_var);
        let env_content = self.generate_env_content(&zv_dir_str, &zv_bin_path_str);

        (env_file, env_content)
    }

    /// Dumps shell specific environment variables to the env file, overwriting if content differs
    /// For CMD and PowerShell, this method does not write to disk as system variables are edited directly
    pub async fn export(&self, app: &App, using_env_var: bool) -> Result<(), ZvError> {
        // Skip file operations for Windows shells that use direct system variable edits
        if self.uses_direct_system_variables() {
            return Ok(());
        }

        let (env_file, content) = self.export_without_dump(app, using_env_var);
        self.write_env_file_if_needed(env_file, &content).await
    }

    /// Helper method to determine path string formatting based on shell and environment
    fn get_path_strings(&self, app: &App, using_env_var: bool) -> (String, String) {
        let zv_dir = app.path();
        let bin_path = app.bin_path();

        if using_env_var {
            // Using ZV_DIR env var, use absolute paths
            self.format_absolute_paths(zv_dir, bin_path)
        } else {
            // Using default path, use ${HOME}/.zv
            self.get_default_path_strings()
        }
    }

    /// Format absolute paths, handling Windows path conversion for Unix-like shells
    fn format_absolute_paths(&self, zv_dir: &Path, bin_path: &Path) -> (String, String) {
        if cfg!(windows) && self.is_unix_shell() {
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
    fn get_default_path_strings(&self) -> (String, String) {
        match self {
            Shell::PowerShell | Shell::Cmd => {
                // Windows shells use different environment variable syntax
                if matches!(self, Shell::PowerShell) {
                    (
                        "$env:HOME\\.zv".to_string(),
                        "$env:HOME\\.zv\\bin".to_string(),
                    )
                } else {
                    (
                        "%USERPROFILE%\\.zv".to_string(),
                        "%USERPROFILE%\\.zv\\bin".to_string(),
                    )
                }
            }
            _ => {
                // Unix-like shells
                ("${HOME}/.zv".to_string(), "${HOME}/.zv/bin".to_string())
            }
        }
    }

    /// Generate shell-specific environment content
    fn generate_env_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        match self {
            Shell::PowerShell => self.generate_powershell_content(zv_dir, zv_bin_path),
            Shell::Cmd => self.generate_cmd_content(zv_dir, zv_bin_path),
            Shell::Fish => self.generate_fish_content(zv_dir, zv_bin_path),
            Shell::Nu => self.generate_nu_content(zv_dir, zv_bin_path),
            Shell::Tcsh => self.generate_tcsh_content(zv_dir, zv_bin_path),
            Shell::Bash | Shell::Zsh | Shell::Posix | Shell::Unknown => {
                if matches!(self, Shell::Unknown) {
                    tracing::warn!("Unknown shell type detected, using POSIX shell syntax");
                }
                self.generate_posix_content(zv_dir, zv_bin_path)
            }
        }
    }

    /// Check if shell uses direct system variable edits (no file writing needed)
    fn uses_direct_system_variables(&self) -> bool {
        matches!(self, Shell::Cmd | Shell::PowerShell)
    }

    /// Write environment file only if content is different or file doesn't exist
    async fn write_env_file_if_needed(
        &self,
        env_file: &Path,
        content: &str,
    ) -> Result<(), ZvError> {
        let should_write = if env_file.exists() {
            match tokio::fs::read_to_string(env_file).await {
                Ok(existing_content) => existing_content.trim() != content.trim(),
                Err(_) => {
                    tracing::warn!("Could not read existing env file, will overwrite");
                    true
                }
            }
        } else {
            true
        };

        if should_write {
            self.write_env_file(env_file, content).await?;
        }

        Ok(())
    }

    /// Write content to environment file
    async fn write_env_file(&self, env_file: &Path, content: &str) -> Result<(), ZvError> {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(env_file)
            .await
            .map_err(|e| {
                ZvError::ZvExportError(eyre!(e).wrap_err(format!(
                    "Failed to open env file for writing: {}",
                    env_file.display()
                )))
            })?;

        file.write_all(content.as_bytes()).await.map_err(|e| {
            ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write to env file"))
        })?;

        file.write_all(b"\n").await.map_err(|e| {
            ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write newline to env file"))
        })?;

        Ok(())
    }

    // Shell-specific content generators
    fn generate_powershell_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        format!(
            r#"# zv shell setup for PowerShell
# To permanently set environment variables in PowerShell, run as Administrator:
# [Environment]::SetEnvironmentVariable("ZV_DIR", "{zv_dir}", "User")
# [Environment]::SetEnvironmentVariable("PATH", "{path};$env:PATH", "User")

$env:ZV_DIR = "{zv_dir}"
if ($env:PATH -notlike "*{path}*") {{
    $env:PATH = "{path};$env:PATH"
}}"#,
            path = zv_bin_path,
            zv_dir = zv_dir
        )
    }

    fn generate_cmd_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        format!(
            r#"REM zv shell setup for Command Prompt
REM To permanently set environment variables in CMD, run as Administrator:
REM setx ZV_DIR "{zv_dir}" /M
REM setx PATH "{path};%PATH%" /M

set "ZV_DIR={zv_dir}"
echo ;%PATH%; | find /i ";{path};" >nul || set "PATH={path};%PATH%""#,
            path = zv_bin_path,
            zv_dir = zv_dir
        )
    }

    fn generate_fish_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        format!(
            r#"#!/usr/bin/env fish
# zv shell setup for Fish shell
set -gx ZV_DIR "{zv_dir}"
if not contains "{path}" $PATH
    set -gx PATH "{path}" $PATH
end"#,
            path = zv_bin_path,
            zv_dir = zv_dir
        )
    }

    fn generate_nu_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        format!(
            r#"# zv shell setup for Nushell
$env.ZV_DIR = "{zv_dir}"
$env.PATH = ($env.PATH | split row (char esep) | prepend "{path}" | uniq)"#,
            path = zv_bin_path,
            zv_dir = zv_dir
        )
    }

    fn generate_tcsh_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        format!(
            r#"#!/bin/csh
# zv shell setup for tcsh/csh
setenv ZV_DIR "{zv_dir}"
echo ":${{PATH}}:" | grep -q ":{path}:" || setenv PATH "{path}:$PATH""#,
            path = zv_bin_path,
            zv_dir = zv_dir
        )
    }

    fn generate_posix_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
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
            path = zv_bin_path,
            zv_dir = zv_dir
        )
    }

    /// Based on current shell type, inspect `path` is in SHELL's PATH
    pub fn check_path_in_system(&self, path: &Path) -> bool {
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
