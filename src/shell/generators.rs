use super::{OsFlavor, Shell, ShellContext, ShellType, path_utils::*};

/// Generate PowerShell environment setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_powershell_content(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::PowerShell,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(unix), // PowerShell on Unix is emulated
        },
    };
    // Default to exporting ZV_DIR for backward compatibility
    shell.generate_env_content(zv_dir, zv_bin_path, true)
}

/// Generate Windows Command Prompt batch script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_cmd_content(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Cmd,
        context: ShellContext {
            target_os: OsFlavor::Windows,
            is_wsl: false,
            is_emulated: false,
        },
    };
    // Default to exporting ZV_DIR for backward compatibility
    shell.generate_env_content(zv_dir, zv_bin_path, true)
}

/// Generate Fish shell setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_fish_content(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Fish,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Fish on Windows is emulated
        },
    };
    // Default to exporting ZV_DIR for backward compatibility
    shell.generate_env_content(zv_dir, zv_bin_path, true)
}

/// Generate Nushell setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_nu_content(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Nu,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Nu on Windows is emulated
        },
    };
    // Default to exporting ZV_DIR for backward compatibility
    shell.generate_env_content(zv_dir, zv_bin_path, true)
}

/// Generate tcsh/csh setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_tcsh_content(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Tcsh,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Tcsh on Windows is emulated
        },
    };
    // Default to exporting ZV_DIR for backward compatibility
    shell.generate_env_content(zv_dir, zv_bin_path, true)
}

/// Generate POSIX-compliant shell setup script (bash, zsh, sh)
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_posix_content(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Bash,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Bash on Windows is emulated
        },
    };
    // Default to exporting ZV_DIR for backward compatibility
    shell.generate_env_content(zv_dir, zv_bin_path, true)
}

/// Generate shell-specific uninstall/cleanup script
/// This function is now a wrapper around the Shell::generate_cleanup_content method
pub fn generate_cleanup_content(shell: &Shell, zv_dir: &str, zv_bin_path: &str) -> String {
    // Default to cleaning up ZV_DIR for backward compatibility
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate PowerShell cleanup script
fn generate_powershell_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::PowerShell,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(unix), // PowerShell on Unix is emulated
        },
    };
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate CMD cleanup script
fn generate_cmd_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Cmd,
        context: ShellContext {
            target_os: OsFlavor::Windows,
            is_wsl: false,
            is_emulated: false,
        },
    };
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate Fish cleanup script
fn generate_fish_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Fish,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Fish on Windows is emulated
        },
    };
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate Nushell cleanup script
fn generate_nu_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Nu,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Nu on Windows is emulated
        },
    };
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate tcsh cleanup script
fn generate_tcsh_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Tcsh,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Tcsh on Windows is emulated
        },
    };
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate POSIX cleanup script
fn generate_posix_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    let shell = Shell {
        shell_type: ShellType::Bash,
        context: ShellContext {
            target_os: if cfg!(target_os = "windows") {
                OsFlavor::Windows
            } else {
                OsFlavor::Unix
            },
            is_wsl: false,
            is_emulated: cfg!(target_os = "windows"), // Bash on Windows is emulated
        },
    };
    shell.generate_cleanup_content(zv_dir, zv_bin_path, true)
}

/// Generate shell-specific instructions for manual setup
/// This function is now a wrapper around the Shell::generate_setup_instructions method
pub fn generate_setup_instructions(shell: &Shell, env_file_path: &str) -> String {
    shell.generate_setup_instructions(env_file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_powershell_content() {
        let content = generate_powershell_content("C:\\zv", "C:\\zv\\bin");
        assert!(content.contains("$env:ZV_DIR"));
        assert!(content.contains("$env:PATH"));
        assert!(content.contains("C:\\zv"));
        assert!(content.contains("C:\\zv\\bin"));
    }

    #[test]
    fn test_generate_fish_content() {
        let content = generate_fish_content("/home/user/.zv", "/home/user/.zv/bin");
        assert!(content.contains("set -gx ZV_DIR"));
        assert!(content.contains("set -gx PATH"));
        assert!(content.contains("/home/user/.zv"));
    }

    #[test]
    fn test_generate_posix_content() {
        let content = generate_posix_content("/home/user/.zv", "/home/user/.zv/bin");
        assert!(content.contains("export ZV_DIR"));
        assert!(content.contains("export PATH"));
        assert!(content.contains("case"));
        assert!(content.contains("/home/user/.zv"));
    }

    #[test]
    fn test_generate_cleanup_content() {
        let shell = Shell {
            shell_type: ShellType::Fish,
            context: ShellContext {
                target_os: OsFlavor::Unix,
                is_wsl: false,
                is_emulated: false,
            },
        };
        let cleanup = generate_cleanup_content(&shell, "/home/user/.zv", "/home/user/.zv/bin");
        assert!(cleanup.contains("set -e ZV_DIR"));
        assert!(cleanup.contains("contains -i"));
    }

    #[test]
    fn test_generate_setup_instructions() {
        let shell = Shell {
            shell_type: ShellType::Bash,
            context: ShellContext {
                target_os: OsFlavor::Unix,
                is_wsl: false,
                is_emulated: false,
            },
        };
        let instructions = generate_setup_instructions(&shell, "/home/user/.zv/env");
        assert!(instructions.contains("source"));
        assert!(instructions.contains("~/.bashrc"));
        assert!(instructions.contains("/home/user/.zv/env"));
    }
}
