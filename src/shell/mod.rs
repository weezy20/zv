use crate::ZvError;
use std::path::{Path, PathBuf};

mod detection;
mod env_export;
mod generators;
mod path_utils;

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
        match self {
            Shell::PowerShell => format!(". \"{}\"", env_file.display()),
            Shell::Fish => format!("source \"{}\"", env_file.display()),
            _ => format!("source \"{}\"", env_file.display()),
        }
    }

    /// Based on current shell type, inspect if `path` is in SHELL's PATH
    pub fn check_path_in_system(&self, path: &Path) -> bool {
        path_utils::check_path_in_system(path)
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
