use crate::ZvError;
use std::path::{Path, PathBuf};

mod detection;
mod env_export;
mod generators;
pub mod path_utils;

pub use detection::detect_shell_from_parent;
pub use env_export::*;
pub use generators::*;
pub use path_utils::*;

impl Default for Shell {
    fn default() -> Self {
        Self::detect()
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
        detection::detect_shell()
    }

    /// Is non-windows shell?
    #[inline]
    pub fn is_unix_shell(&self) -> bool {
        !matches!(self, Shell::Cmd | Shell::PowerShell)
    }

    /// Is Windows shell?
    #[inline]
    pub fn is_windows_shell(&self) -> bool {
        matches!(self, Shell::Cmd | Shell::PowerShell)
    }

    /// Is WSL shell? (Unix shell running on Windows through WSL)
    /// This is detected when the target OS is Windows but the shell is Unix-like
    pub fn is_wsl_shell(&self) -> bool {
        cfg!(target_os = "windows")
            && !self.is_windows_shell()
            && (std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok())
    }

    /// Is Unix shell running on Windows? (includes WSL, GitBash, MinGW, etc.)
    /// This is a broader check for any Unix-like shell on Windows
    pub fn is_unix_shell_in_windows(&self) -> bool {
        cfg!(target_os = "windows") && !self.is_windows_shell()
    }

    /// Is PowerShell running on Unix/Linux?
    /// This is the edge case where PowerShell is installed and used on Linux/macOS
    pub fn is_powershell_in_unix(&self) -> bool {
        cfg!(unix) && matches!(self, Shell::PowerShell)
    }

    /// Get the appropriate PATH separator for the shell
    pub fn get_path_separator(self: &Shell) -> char {
        // Powershell is available on Unix so we must exclude that from is_windows_shell()
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
        match self {
            Shell::PowerShell => {
                // PowerShell on Unix should use Unix-style env file
                if self.is_powershell_in_unix() {
                    "env"
                } else {
                    "env.ps1"
                }
            }
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
            Shell::PowerShell => {
                // PowerShell on Unix should use Unix-style RC files
                if self.is_powershell_in_unix() {
                    vec![home_dir.join(".profile")]
                } else {
                    vec![] // Windows PowerShell doesn't use RC files
                }
            }
            Shell::Cmd => vec![], // Windows CMD doesn't use RC files
        }
    }

    /// Generate the source command for this shell type
    pub fn get_source_command(&self, env_file: &Path) -> String {
        match self {
            Shell::PowerShell => format!(". \"{}\"", env_file.display()),
            Shell::Fish => format!("source \"{}\"", env_file.display()),
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

        let template = match self {
            Shell::PowerShell => {
                // Always use PowerShell syntax for PowerShell shell
                // The path separator will be adjusted based on platform
                include_str!("env_files/env.ps1")
            }
            Shell::Cmd => include_str!("env_files/env.bat"),
            Shell::Fish => include_str!("env_files/env.fish"),
            Shell::Nu => include_str!("env_files/env.nu"),
            Shell::Tcsh => include_str!("env_files/env.csh"),
            Shell::Bash | Shell::Zsh | Shell::Posix | Shell::Unknown => {
                if matches!(self, Shell::Unknown) {
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

        let template = match self {
            Shell::PowerShell => {
                // Always use PowerShell syntax for PowerShell shell
                // The path separator will be adjusted based on platform
                include_str!("env_files/cleanup/cleanup.ps1")
            }
            Shell::Cmd => include_str!("env_files/cleanup/cleanup.bat"),
            Shell::Fish => include_str!("env_files/cleanup/cleanup.fish"),
            Shell::Nu => include_str!("env_files/cleanup/cleanup.nu"),
            Shell::Tcsh => include_str!("env_files/cleanup/cleanup.csh"),
            Shell::Bash | Shell::Zsh | Shell::Posix | Shell::Unknown => {
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
        let template = match self {
            Shell::PowerShell => include_str!("env_files/setup_instructions/powershell.txt"),
            Shell::Cmd => include_str!("env_files/setup_instructions/cmd.txt"),
            Shell::Fish => include_str!("env_files/setup_instructions/fish.txt"),
            Shell::Nu => include_str!("env_files/setup_instructions/nu.txt"),
            Shell::Bash => include_str!("env_files/setup_instructions/bash.txt"),
            Shell::Zsh => include_str!("env_files/setup_instructions/zsh.txt"),
            Shell::Tcsh => include_str!("env_files/setup_instructions/tcsh.txt"),
            Shell::Posix | Shell::Unknown => include_str!("env_files/setup_instructions/default.txt"),
        };

        template.replace("{env_file_path}", env_file_path)
    }
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::PowerShell => "powershell",
            Shell::Fish => "fish",
            Shell::Cmd => "cmd",
            Shell::Tcsh => "tcsh",
            Shell::Posix => "posix",
            Shell::Nu => "nu",
            Shell::Unknown => "unknown",
        };
        write!(f, "{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_windows_shell() {
        assert!(Shell::PowerShell.is_windows_shell());
        assert!(Shell::Cmd.is_windows_shell());

        assert!(!Shell::Bash.is_windows_shell());
        assert!(!Shell::Zsh.is_windows_shell());
        assert!(!Shell::Fish.is_windows_shell());
    }

    #[test]
    fn test_is_unix_shell() {
        assert!(!Shell::PowerShell.is_unix_shell());
        assert!(!Shell::Cmd.is_unix_shell());

        assert!(Shell::Bash.is_unix_shell());
        assert!(Shell::Zsh.is_unix_shell());
        assert!(Shell::Fish.is_unix_shell());
        assert!(Shell::Tcsh.is_unix_shell());
        assert!(Shell::Posix.is_unix_shell());
        assert!(Shell::Nu.is_unix_shell());
    }

    #[test]
    fn test_unix_shell_in_windows_relationship() {
        // is_unix_shell_in_windows should be a superset of is_wsl_shell
        // All WSL shells should be Unix shells on Windows, but not all Unix shells on Windows are WSL

        // On Windows target with Unix shells, is_unix_shell_in_windows should be true
        if cfg!(target_os = "windows") {
            assert!(Shell::Bash.is_unix_shell_in_windows());
            assert!(Shell::Zsh.is_unix_shell_in_windows());
            assert!(Shell::Fish.is_unix_shell_in_windows());

            // Windows shells should not be Unix shells on Windows
            assert!(!Shell::PowerShell.is_unix_shell_in_windows());
            assert!(!Shell::Cmd.is_unix_shell_in_windows());
        } else {
            // On non-Windows targets, is_unix_shell_in_windows should always be false
            assert!(!Shell::Bash.is_unix_shell_in_windows());
            assert!(!Shell::Zsh.is_unix_shell_in_windows());
            assert!(!Shell::PowerShell.is_unix_shell_in_windows());
        }
    }

    #[test]
    fn test_is_powershell_in_unix() {
        // On Unix targets, PowerShell should be detected as PowerShell on Unix
        if cfg!(unix) {
            assert!(Shell::PowerShell.is_powershell_in_unix());
            // Other shells should not be PowerShell on Unix
            assert!(!Shell::Bash.is_powershell_in_unix());
            assert!(!Shell::Zsh.is_powershell_in_unix());
            assert!(!Shell::Fish.is_powershell_in_unix());
            assert!(!Shell::Cmd.is_powershell_in_unix());
        } else {
            // On non-Unix targets, is_powershell_in_unix should always be false
            assert!(!Shell::PowerShell.is_powershell_in_unix());
            assert!(!Shell::Bash.is_powershell_in_unix());
            assert!(!Shell::Zsh.is_powershell_in_unix());
        }
    }
}
