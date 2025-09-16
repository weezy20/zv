#![allow(unused)]

use std::path::{Path, PathBuf};

mod detection;
mod env_export;
mod generators;
pub mod path_utils;
pub mod setup;

pub use detection::detect_shell_from_parent;
pub use generators::*;
pub use path_utils::*;
pub use setup::*;

impl Default for Shell {
    fn default() -> Self {
        Self::detect()
    }
}

/// Shell type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
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

/// Operating system flavor for cross-platform handling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsFlavor {
    Windows,
    Unix,
}

/// Shell context information for enhanced shell detection
#[derive(Debug, Clone)]
pub struct ShellContext {
    /// Target operating system
    pub target_os: OsFlavor,
    /// Whether running in WSL environment
    pub is_wsl: bool,
    /// Whether running in emulated environment (GitBash, MinGW, etc.)
    pub is_emulated: bool,
}

/// Enhanced Shell struct with type and context information
#[derive(Debug, Clone)]
pub struct Shell {
    /// The type of shell
    pub shell_type: ShellType,
    /// Context information about the shell environment
    pub context: ShellContext,
}

impl Shell {
    /// Detect shell from environment with enhanced context
    pub fn detect() -> Shell {
        let shell_type = detection::detect_shell();
        let context = ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: std::env::var("WSL_DISTRO_NAME").is_ok()
                || std::env::var("WSL_INTEROP").is_ok(),
            is_emulated: Self::detect_emulated_shell(&shell_type),
        };

        Shell {
            shell_type,
            context,
        }
    }

    /// Detect if shell is running in an emulated environment
    fn detect_emulated_shell(shell_type: &ShellType) -> bool {
        // PowerShell on Unix is emulated
        if cfg!(unix) && matches!(shell_type, ShellType::PowerShell) {
            return true;
        }

        // Unix shells on Windows (except WSL) are emulated
        if cfg!(target_os = "windows")
            && !matches!(shell_type, ShellType::Cmd | ShellType::PowerShell)
        {
            // Check if it's not WSL
            let is_wsl =
                std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok();
            return !is_wsl;
        }

        false
    }

    /// Is non-windows shell?
    #[inline]
    pub fn is_unix_shell(&self) -> bool {
        !matches!(self.shell_type, ShellType::Cmd | ShellType::PowerShell)
    }

    /// Is Windows shell?
    #[inline]
    pub fn is_windows_shell(&self) -> bool {
        matches!(self.shell_type, ShellType::Cmd | ShellType::PowerShell)
    }

    /// Is WSL shell? (Unix shell running on Windows through WSL)
    pub fn is_wsl_shell(&self) -> bool {
        self.context.is_wsl && self.is_unix_shell()
    }

    /// Is Unix shell running on Windows? (includes WSL, GitBash, MinGW, etc.)
    pub fn is_unix_shell_in_windows(&self) -> bool {
        matches!(self.context.target_os, OsFlavor::Windows) && self.is_unix_shell()
    }

    /// Is PowerShell running on Unix/Linux?
    pub fn is_powershell_in_unix(&self) -> bool {
        matches!(self.context.target_os, OsFlavor::Unix)
            && matches!(self.shell_type, ShellType::PowerShell)
    }

    /// Is shell running in an emulated environment?
    pub fn is_emulated(&self) -> bool {
        self.context.is_emulated
    }

    /// Get the appropriate home directory for this shell context
    /// Handles emulated Unix shells on Windows that need Windows user profile
    pub fn get_home_dir(&self) -> Option<PathBuf> {
        if self.is_unix_shell_in_windows() && !self.is_wsl_shell() {
            // For emulated Unix shells on Windows (GitBash, MinGW, etc.)
            // Use the actual Windows user profile directory
            self.get_windows_user_profile_dir()
        } else {
            // For native shells (Unix on Unix, Windows shells on Windows, WSL)
            dirs::home_dir()
        }
    }

    /// Get the Windows user profile directory for emulated shells
    fn get_windows_user_profile_dir(&self) -> Option<PathBuf> {
        cfg_if::cfg_if! {
            if #[cfg(windows)] {
                // Try USERPROFILE first (most reliable on Windows)
                if let Ok(userprofile) = std::env::var("USERPROFILE") {
                    return Some(PathBuf::from(userprofile));
                }

                // Fallback to HOMEDRIVE + HOMEPATH
                if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
                    return Some(PathBuf::from(format!("{}{}", drive, path)));
                }

                // Last resort: use dirs crate
                dirs::home_dir()
            } else {
                // This should never be called on non-Windows, but provide a safe fallback
                dirs::home_dir()
            }
        }
    }

    /// Get the appropriate PATH separator for the shell
    pub fn get_path_separator(&self) -> char {
        // PowerShell is available on Unix so we must exclude that from is_windows_shell()
        if self.is_windows_shell() && !self.is_powershell_in_unix() {
            ';'
        } else {
            // Unix shells and Unix shells on Windows (WSL, GitBash, etc.) use colon separator
            // This includes PowerShell on Unix
            ':'
        }
    }

    /// Get the appropriate env file name based on shell type
    pub fn env_file_name(&self) -> &'static str {
        match self.shell_type {
            ShellType::PowerShell => {
                // PowerShell on Unix should use Unix-style env file
                if self.is_powershell_in_unix() {
                    "env"
                } else {
                    "env.ps1"
                }
            }
            ShellType::Cmd => "env.bat",
            ShellType::Fish => "env.fish",
            ShellType::Nu => "env.nu",
            ShellType::Tcsh => "env.csh",
            _ => "env", // bash, zsh, and other POSIX shells
        }
    }

    /// Get shell RC files that should be modified for this shell type
    pub fn get_rc_files(&self) -> Vec<PathBuf> {
        let home_dir = match self.get_home_dir() {
            Some(dir) => dir,
            None => return vec![],
        };
        let rc_file = |name: &'static str| -> PathBuf { home_dir.join(name) };
        match self.shell_type {
            ShellType::Bash => vec![
                rc_file(".bashrc"),
                rc_file(".bash_profile"),
                rc_file(".profile"),
            ],
            ShellType::Zsh => {
                // For zsh, check ZDOTDIR environment variable for .zshenv location
                let zshenv = match std::env::var("ZDOTDIR") {
                    Ok(zdotdir) if !zdotdir.is_empty() => PathBuf::from(zdotdir).join(".zshenv"),
                    _ => rc_file(".zshenv"),
                };
                vec![zshenv, rc_file(".zshrc"), rc_file(".zprofile")]
            }
            ShellType::Fish => {
                // For fish, check XDG_CONFIG_HOME first, then fall back to ~/.config
                // Use conf.d directory as recommended by fish shell documentation
                let mut fish_files = Vec::new();

                // Try XDG_CONFIG_HOME/fish/conf.d/zv.fish
                if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
                    if !xdg_config.is_empty() {
                        fish_files.push(PathBuf::from(xdg_config).join("fish/conf.d/zv.fish"));
                    }
                }

                // Always include ~/.config/fish/conf.d/zv.fish as fallback
                fish_files.push(rc_file(".config/fish/conf.d/zv.fish"));

                fish_files
            }
            ShellType::Tcsh => vec![rc_file(".tcshrc"), rc_file(".cshrc"), rc_file(".profile")],
            ShellType::Nu => {
                // For nushell, check XDG_CONFIG_HOME first, then fall back to ~/.config
                let mut nu_files = Vec::new();

                // Try XDG_CONFIG_HOME/nushell/config.nu
                if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
                    if !xdg_config.is_empty() {
                        nu_files.push(PathBuf::from(xdg_config).join("nushell/config.nu"));
                    }
                }

                // Always include ~/.config/nushell/config.nu as fallback
                nu_files.push(rc_file(".config/nushell/config.nu"));

                nu_files
            }
            ShellType::Posix | ShellType::Unknown => {
                vec![rc_file(".bash_profile"), rc_file(".profile")]
            }
            ShellType::PowerShell => {
                // PowerShell on Unix should use Unix-style RC files
                if self.is_powershell_in_unix() {
                    std::env::var_os("PROFILE")
                        .map(PathBuf::from)
                        .into_iter()
                        .chain(vec![rc_file(".bashrc"), rc_file(".profile")])
                        .collect::<Vec<PathBuf>>()
                } else {
                    std::env::var_os("PROFILE")
                        .map(PathBuf::from)
                        .into_iter()
                        .collect()
                }
            }
            ShellType::Cmd => vec![], // Windows CMD doesn't use RC files
        }
    }

    /// Generate the source command for this shell type
    pub fn get_source_command(&self, env_file: &Path) -> String {
        match self.shell_type {
            ShellType::PowerShell => format!(". \"{}\"", env_file.display()),
            ShellType::Fish => format!("source \"{}\"", env_file.display()),
            _ => format!("source \"{}\"", env_file.display()),
        }
    }

    /// Generate shell-specific environment content using templates
    pub fn generate_env_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        use crate::shell::path_utils::escape_path_for_shell;

        // Escape paths for shell-specific safety
        let escaped_zv_dir = escape_path_for_shell(self, zv_dir);
        let escaped_bin_path = escape_path_for_shell(self, zv_bin_path);
        let path_separator = self.get_path_separator();

        let template = match self.shell_type {
            ShellType::PowerShell => {
                // Always use PowerShell syntax for PowerShell shell
                // The path separator will be adjusted based on platform
                include_str!("env_files/env.ps1")
            }
            ShellType::Cmd => include_str!("env_files/env.bat"),
            ShellType::Fish => include_str!("env_files/env.fish"),
            ShellType::Nu => include_str!("env_files/env.nu"),
            ShellType::Tcsh => include_str!("env_files/env.csh"),
            ShellType::Bash | ShellType::Zsh | ShellType::Posix | ShellType::Unknown => {
                if matches!(self.shell_type, ShellType::Unknown) {
                    tracing::warn!("Unknown shell type detected, using POSIX shell syntax");
                }
                include_str!("env_files/env.sh")
            }
        };

        template
            .replace("{zv_dir}", &escaped_zv_dir)
            .replace("{zv_bin_path}", &escaped_bin_path)
            .replace("{zv_path_separator}", &path_separator.to_string())
    }

    /// Generate shell-specific cleanup content using templates
    pub fn generate_cleanup_content(&self, zv_dir: &str, zv_bin_path: &str) -> String {
        use crate::shell::path_utils::escape_path_for_shell;

        // Escape paths for shell-specific safety
        let escaped_zv_dir = escape_path_for_shell(self, zv_dir);
        let escaped_bin_path = escape_path_for_shell(self, zv_bin_path);
        let path_separator = self.get_path_separator();

        let template = match self.shell_type {
            ShellType::PowerShell => {
                // Always use PowerShell syntax for PowerShell shell
                // The path separator will be adjusted based on platform
                include_str!("env_files/cleanup/cleanup.ps1")
            }
            ShellType::Cmd => include_str!("env_files/cleanup/cleanup.bat"),
            ShellType::Fish => include_str!("env_files/cleanup/cleanup.fish"),
            ShellType::Nu => include_str!("env_files/cleanup/cleanup.nu"),
            ShellType::Tcsh => include_str!("env_files/cleanup/cleanup.csh"),
            ShellType::Bash | ShellType::Zsh | ShellType::Posix | ShellType::Unknown => {
                include_str!("env_files/cleanup/cleanup.sh")
            }
        };

        template
            .replace("{zv_dir}", &escaped_zv_dir)
            .replace("{zv_bin_path}", &escaped_bin_path)
            .replace("{zv_path_separator}", &path_separator.to_string())
    }

    /// Generate shell-specific setup instructions using templates
    pub fn generate_setup_instructions(&self, env_file_path: &str) -> String {
        let template = match self.shell_type {
            ShellType::PowerShell => include_str!("env_files/setup_instructions/powershell.txt"),
            ShellType::Cmd => include_str!("env_files/setup_instructions/cmd.txt"),
            ShellType::Fish => include_str!("env_files/setup_instructions/fish.txt"),
            ShellType::Nu => include_str!("env_files/setup_instructions/nu.txt"),
            ShellType::Bash => include_str!("env_files/setup_instructions/bash.txt"),
            ShellType::Zsh => include_str!("env_files/setup_instructions/zsh.txt"),
            ShellType::Tcsh => include_str!("env_files/setup_instructions/tcsh.txt"),
            ShellType::Posix | ShellType::Unknown => {
                include_str!("env_files/setup_instructions/default.txt")
            }
        };

        template.replace("{env_file_path}", env_file_path)
    }
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self.shell_type {
            ShellType::Bash => "bash",
            ShellType::Zsh => "zsh",
            ShellType::PowerShell => "powershell",
            ShellType::Fish => "fish",
            ShellType::Cmd => "cmd",
            ShellType::Tcsh => "tcsh",
            ShellType::Posix => "posix",
            ShellType::Nu => "nu",
            ShellType::Unknown => "unknown",
        };
        write!(f, "{}", name)
    }
}

impl std::fmt::Display for ShellType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ShellType::Bash => "bash",
            ShellType::Zsh => "zsh",
            ShellType::PowerShell => "powershell",
            ShellType::Fish => "fish",
            ShellType::Cmd => "cmd",
            ShellType::Tcsh => "tcsh",
            ShellType::Posix => "posix",
            ShellType::Nu => "nu",
            ShellType::Unknown => "unknown",
        };
        write!(f, "{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_shell(
        shell_type: ShellType,
        target_os: OsFlavor,
        is_wsl: bool,
        is_emulated: bool,
    ) -> Shell {
        Shell {
            shell_type,
            context: ShellContext {
                target_os,
                is_wsl,
                is_emulated,
            },
        }
    }

    #[test]
    fn test_is_windows_shell() {
        let powershell_win =
            create_test_shell(ShellType::PowerShell, OsFlavor::Windows, false, false);
        let cmd_win = create_test_shell(ShellType::Cmd, OsFlavor::Windows, false, false);
        let bash_win = create_test_shell(ShellType::Bash, OsFlavor::Windows, false, true);
        let zsh_unix = create_test_shell(ShellType::Zsh, OsFlavor::Unix, false, false);

        assert!(powershell_win.is_windows_shell());
        assert!(cmd_win.is_windows_shell());
        assert!(!bash_win.is_windows_shell());
        assert!(!zsh_unix.is_windows_shell());
    }

    #[test]
    fn test_is_unix_shell() {
        let powershell_win =
            create_test_shell(ShellType::PowerShell, OsFlavor::Windows, false, false);
        let cmd_win = create_test_shell(ShellType::Cmd, OsFlavor::Windows, false, false);
        let bash_unix = create_test_shell(ShellType::Bash, OsFlavor::Unix, false, false);
        let zsh_unix = create_test_shell(ShellType::Zsh, OsFlavor::Unix, false, false);
        let fish_unix = create_test_shell(ShellType::Fish, OsFlavor::Unix, false, false);

        assert!(!powershell_win.is_unix_shell());
        assert!(!cmd_win.is_unix_shell());
        assert!(bash_unix.is_unix_shell());
        assert!(zsh_unix.is_unix_shell());
        assert!(fish_unix.is_unix_shell());
    }

    #[test]
    fn test_is_wsl_shell() {
        let bash_wsl = create_test_shell(ShellType::Bash, OsFlavor::Windows, true, false);
        let bash_gitbash = create_test_shell(ShellType::Bash, OsFlavor::Windows, false, true);
        let bash_unix = create_test_shell(ShellType::Bash, OsFlavor::Unix, false, false);
        let powershell_win =
            create_test_shell(ShellType::PowerShell, OsFlavor::Windows, false, false);

        assert!(bash_wsl.is_wsl_shell());
        assert!(!bash_gitbash.is_wsl_shell()); // GitBash is emulated but not WSL
        assert!(!bash_unix.is_wsl_shell());
        assert!(!powershell_win.is_wsl_shell()); // PowerShell is not Unix shell
    }

    #[test]
    fn test_unix_shell_in_windows() {
        let bash_wsl = create_test_shell(ShellType::Bash, OsFlavor::Windows, true, false);
        let bash_gitbash = create_test_shell(ShellType::Bash, OsFlavor::Windows, false, true);
        let bash_unix = create_test_shell(ShellType::Bash, OsFlavor::Unix, false, false);
        let powershell_win =
            create_test_shell(ShellType::PowerShell, OsFlavor::Windows, false, false);

        assert!(bash_wsl.is_unix_shell_in_windows());
        assert!(bash_gitbash.is_unix_shell_in_windows());
        assert!(!bash_unix.is_unix_shell_in_windows());
        assert!(!powershell_win.is_unix_shell_in_windows());
    }

    #[test]
    fn test_is_powershell_in_unix() {
        let powershell_unix = create_test_shell(ShellType::PowerShell, OsFlavor::Unix, false, true);
        let powershell_win =
            create_test_shell(ShellType::PowerShell, OsFlavor::Windows, false, false);
        let bash_unix = create_test_shell(ShellType::Bash, OsFlavor::Unix, false, false);

        assert!(powershell_unix.is_powershell_in_unix());
        assert!(!powershell_win.is_powershell_in_unix());
        assert!(!bash_unix.is_powershell_in_unix());
    }

    #[test]
    fn test_is_emulated() {
        let bash_gitbash = create_test_shell(ShellType::Bash, OsFlavor::Windows, false, true);
        let powershell_unix = create_test_shell(ShellType::PowerShell, OsFlavor::Unix, false, true);
        let bash_unix = create_test_shell(ShellType::Bash, OsFlavor::Unix, false, false);
        let powershell_win =
            create_test_shell(ShellType::PowerShell, OsFlavor::Windows, false, false);

        assert!(bash_gitbash.is_emulated());
        assert!(powershell_unix.is_emulated());
        assert!(!bash_unix.is_emulated());
        assert!(!powershell_win.is_emulated());
    }
}
