use crate::shell::{Shell, ShellType};
use std::path::{Path, PathBuf};
use yansi::Paint;

/// Information about files modified during setup
#[derive(Debug, Clone)]
pub struct ModifiedFile {
    pub file_type: FileType,
    pub path: PathBuf,
    pub action: FileAction,
}

#[derive(Debug, Clone)]
pub enum FileType {
    RcFile,
    EnvironmentFile,
    RegistryEntry,
}

#[derive(Debug, Clone)]
pub enum FileAction {
    Created,
    Modified,
    SourceAdded,
}

/// Post-setup instructions with shell-specific source commands
#[derive(Debug, Clone)]
pub struct PostSetupInstructions {
    pub shell_type: ShellType,
    pub modified_files: Vec<ModifiedFile>,
    pub primary_source_command: String,
    pub alternative_instructions: Vec<String>,
}

impl PostSetupInstructions {
    /// Generate post-setup instructions for a shell based on modified files
    pub fn generate_for_shell(shell: &Shell, modified_files: Vec<ModifiedFile>) -> Self {
        let primary_source_command = Self::get_primary_source_command(shell, &modified_files);
        let alternative_instructions = Self::generate_alternatives(shell, &modified_files);

        Self {
            shell_type: shell.shell_type,
            modified_files,
            primary_source_command,
            alternative_instructions,
        }
    }

    /// Get the primary source command based on shell type and modified files
    fn get_primary_source_command(shell: &Shell, modified_files: &[ModifiedFile]) -> String {
        // Priority: RC file > Environment file
        for file in modified_files {
            match file.file_type {
                FileType::RcFile => {
                    return Self::format_source_command(shell, &file.path);
                }
                _ => continue,
            }
        }

        // Fallback to environment file
        for file in modified_files {
            match file.file_type {
                FileType::EnvironmentFile => {
                    return Self::format_source_command(shell, &file.path);
                }
                _ => continue,
            }
        }

        // Fallback to generic restart message
        "Restart your shell to apply changes".to_string()
    }

    /// Format a source command for the specific shell type
    fn format_source_command(shell: &Shell, file_path: &Path) -> String {
        match shell.shell_type {
            ShellType::PowerShell => {
                format!(". \"{}\"", file_path.display())
            }
            ShellType::Fish => {
                format!("source \"{}\"", file_path.display())
            }
            _ => {
                // POSIX-compliant shells (bash, zsh, etc.)
                format!("source \"{}\"", file_path.display())
            }
        }
    }

    /// Generate alternative instructions for the shell
    fn generate_alternatives(shell: &Shell, modified_files: &[ModifiedFile]) -> Vec<String> {
        let mut alternatives = Vec::new();

        // Add restart terminal option
        alternatives.push("Restart your terminal".to_string());

        // Add shell-specific alternatives based on modified files
        for file in modified_files {
            match file.file_type {
                FileType::RcFile => {
                    // Don't duplicate the primary command
                    continue;
                }
                FileType::EnvironmentFile => {
                    let source_cmd = Self::format_source_command(shell, &file.path);
                    if !alternatives.iter().any(|alt| alt.contains(&source_cmd)) {
                        alternatives.push(source_cmd);
                    }
                }
                FileType::RegistryEntry => {
                    if shell.is_windows_shell() && !shell.is_powershell_in_unix() {
                        alternatives
                            .push("Run 'refreshenv' to reload environment variables".to_string());
                    }
                }
            }
        }

        alternatives
    }

    /// Display the post-setup instructions
    pub fn display(&self) {
        println!();
        println!("{}", Paint::yellow("→ Next steps:"));

        // Show primary instruction
        println!("• {}", self.primary_source_command);

        // Show verification step
        println!("• Run 'zv --version' to verify the setup");

        // Show alternatives if available
        if self.alternative_instructions.len() > 1 {
            println!();
            println!("{}", Paint::dim("Alternative options:"));
            for (i, instruction) in self.alternative_instructions.iter().enumerate() {
                if i == 0 {
                    continue; // Skip the first one as it's usually "restart terminal"
                }
                println!("  • {}", Paint::dim(instruction));
            }
        }
    }
}

/// Create modified file entry for RC file
pub fn create_rc_file_entry(path: PathBuf, action: FileAction) -> ModifiedFile {
    ModifiedFile {
        file_type: FileType::RcFile,
        path,
        action,
    }
}

/// Create modified file entry for environment file
pub fn create_env_file_entry(path: PathBuf, action: FileAction) -> ModifiedFile {
    ModifiedFile {
        file_type: FileType::EnvironmentFile,
        path,
        action,
    }
}

/// Create modified file entry for registry entry
pub fn create_registry_entry() -> ModifiedFile {
    ModifiedFile {
        file_type: FileType::RegistryEntry,
        path: PathBuf::from("HKEY_CURRENT_USER\\Environment\\PATH"),
        action: FileAction::Modified,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{OsFlavor, ShellContext};

    fn create_test_shell(shell_type: ShellType) -> Shell {
        Shell {
            shell_type,
            context: ShellContext {
                target_os: OsFlavor::Unix,
                is_wsl: false,
                is_emulated: false,
            },
        }
    }

    #[test]
    fn test_post_setup_instructions_bash_rc_file() {
        let shell = create_test_shell(ShellType::Bash);
        let modified_files = vec![create_rc_file_entry(
            PathBuf::from("/home/user/.bashrc"),
            FileAction::SourceAdded,
        )];

        let instructions = PostSetupInstructions::generate_for_shell(&shell, modified_files);

        assert_eq!(
            instructions.primary_source_command,
            "source \"/home/user/.bashrc\""
        );
        assert_eq!(instructions.shell_type, ShellType::Bash);
    }

    #[test]
    fn test_post_setup_instructions_zsh_env_file() {
        let shell = create_test_shell(ShellType::Zsh);
        let modified_files = vec![create_env_file_entry(
            PathBuf::from("/home/user/.zv/env"),
            FileAction::Created,
        )];

        let instructions = PostSetupInstructions::generate_for_shell(&shell, modified_files);

        assert_eq!(
            instructions.primary_source_command,
            "source \"/home/user/.zv/env\""
        );
        assert_eq!(instructions.shell_type, ShellType::Zsh);
    }

    #[test]
    fn test_post_setup_instructions_fish() {
        let shell = create_test_shell(ShellType::Fish);
        let modified_files = vec![create_rc_file_entry(
            PathBuf::from("/home/user/.config/fish/config.fish"),
            FileAction::SourceAdded,
        )];

        let instructions = PostSetupInstructions::generate_for_shell(&shell, modified_files);

        assert_eq!(
            instructions.primary_source_command,
            "source \"/home/user/.config/fish/config.fish\""
        );
        assert_eq!(instructions.shell_type, ShellType::Fish);
    }

    #[test]
    fn test_post_setup_instructions_powershell() {
        let shell = create_test_shell(ShellType::PowerShell);
        let modified_files = vec![create_env_file_entry(
            PathBuf::from("/home/user/.zv/env.ps1"),
            FileAction::Created,
        )];

        let instructions = PostSetupInstructions::generate_for_shell(&shell, modified_files);

        assert_eq!(
            instructions.primary_source_command,
            ". \"/home/user/.zv/env.ps1\""
        );
        assert_eq!(instructions.shell_type, ShellType::PowerShell);
    }

    #[test]
    fn test_post_setup_instructions_priority_rc_over_env() {
        let shell = create_test_shell(ShellType::Bash);
        let modified_files = vec![
            create_env_file_entry(PathBuf::from("/home/user/.zv/env"), FileAction::Created),
            create_rc_file_entry(PathBuf::from("/home/user/.bashrc"), FileAction::SourceAdded),
        ];

        let instructions = PostSetupInstructions::generate_for_shell(&shell, modified_files);

        // Should prioritize RC file over environment file
        assert_eq!(
            instructions.primary_source_command,
            "source \"/home/user/.bashrc\""
        );
    }

    #[test]
    fn test_post_setup_instructions_no_files() {
        let shell = create_test_shell(ShellType::Bash);
        let modified_files = vec![];

        let instructions = PostSetupInstructions::generate_for_shell(&shell, modified_files);

        assert_eq!(
            instructions.primary_source_command,
            "Restart your shell to apply changes"
        );
    }
}
