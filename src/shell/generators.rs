use super::{Shell, path_utils::*};

/// Generate shell-specific environment content with proper path escaping
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_env_content(shell: &Shell, zv_dir: &str, zv_bin_path: &str) -> String {
    shell.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate PowerShell environment setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_powershell_content(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::PowerShell.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate Windows Command Prompt batch script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_cmd_content(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Cmd.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate Fish shell setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_fish_content(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Fish.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate Nushell setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_nu_content(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Nu.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate tcsh/csh setup script
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_tcsh_content(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Tcsh.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate POSIX-compliant shell setup script (bash, zsh, sh)
/// This function is now a wrapper around the Shell::generate_env_content method
pub fn generate_posix_content(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Bash.generate_env_content(zv_dir, zv_bin_path)
}

/// Generate shell-specific uninstall/cleanup script
/// This function is now a wrapper around the Shell::generate_cleanup_content method
pub fn generate_cleanup_content(shell: &Shell, zv_dir: &str, zv_bin_path: &str) -> String {
    shell.generate_cleanup_content(zv_dir, zv_bin_path)
}

/// Generate PowerShell cleanup script
fn generate_powershell_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::PowerShell.generate_cleanup_content(zv_dir, zv_bin_path)
}

/// Generate CMD cleanup script
fn generate_cmd_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Cmd.generate_cleanup_content(zv_dir, zv_bin_path)
}

/// Generate Fish cleanup script
fn generate_fish_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Fish.generate_cleanup_content(zv_dir, zv_bin_path)
}

/// Generate Nushell cleanup script
fn generate_nu_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Nu.generate_cleanup_content(zv_dir, zv_bin_path)
}

/// Generate tcsh cleanup script
fn generate_tcsh_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Tcsh.generate_cleanup_content(zv_dir, zv_bin_path)
}

/// Generate POSIX cleanup script
fn generate_posix_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    Shell::Bash.generate_cleanup_content(zv_dir, zv_bin_path)
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
        let cleanup =
            generate_cleanup_content(&Shell::Fish, "/home/user/.zv", "/home/user/.zv/bin");
        assert!(cleanup.contains("set -e ZV_DIR"));
        assert!(cleanup.contains("contains -i"));
    }

    #[test]
    fn test_generate_setup_instructions() {
        let instructions = generate_setup_instructions(&Shell::Bash, "/home/user/.zv/env");
        assert!(instructions.contains("source"));
        assert!(instructions.contains("~/.bashrc"));
        assert!(instructions.contains("/home/user/.zv/env"));
    }
}
