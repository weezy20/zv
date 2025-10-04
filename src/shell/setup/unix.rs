use crate::shell::{Shell, ShellType};
use std::path::{Path, PathBuf};
use yansi::Paint;

const TARGET: &str = "zv::shell::setup::unix";

/// Select the appropriate RC file for the shell with shell-specific preferences
pub fn select_rc_file(shell: &Shell) -> PathBuf {
    let home_dir = match dirs::home_dir() {
        Some(dir) => dir,
        None => {
            // Fallback to .profile if home directory cannot be determined
            return PathBuf::from(".profile");
        }
    };
    match shell.shell_type {
        ShellType::Bash => {
            // Bash preference order: .bashrc (interactive), .bash_profile (login), .profile (fallback)
            let bashrc = home_dir.join(".bashrc");
            if bashrc.exists() {
                return bashrc;
            }

            let bash_profile = home_dir.join(".bash_profile");
            if bash_profile.exists() {
                return bash_profile;
            }

            home_dir.join(".profile")
        }
        ShellType::Zsh => {
            // Zsh preference order: .zshenv (always sourced), .zshrc (interactive), .zprofile (login)
            let zshenv = home_dir.join(".zshenv");
            if zshenv.exists() {
                return zshenv;
            }

            let zshrc = home_dir.join(".zshrc");
            if zshrc.exists() {
                return zshrc;
            }

            let zprofile = home_dir.join(".zprofile");
            if zprofile.exists() {
                return zprofile;
            }

            // Default to .zshenv for new installations
            home_dir.join(".zshenv")
        }
        ShellType::Fish => {
            // Fish uses config.fish in the config directory
            let config_dir = home_dir.join(".config/fish");
            config_dir.join("config.fish")
        }
        ShellType::Tcsh => {
            // Tcsh preference order: .tcshrc, .cshrc, .profile
            let tcshrc = home_dir.join(".tcshrc");
            if tcshrc.exists() {
                return tcshrc;
            }

            let cshrc = home_dir.join(".cshrc");
            if cshrc.exists() {
                return cshrc;
            }

            home_dir.join(".profile")
        }
        ShellType::Nu => {
            // Nushell uses config.nu in the config directory
            let config_dir = home_dir.join(".config/nushell");
            config_dir.join("config.nu")
        }
        ShellType::PowerShell => {
            // PowerShell on Unix should use .profile
            if shell.is_powershell_in_unix() {
                home_dir.join(".profile")
            } else {
                // This shouldn't happen for Unix setup, but provide a fallback
                home_dir.join(".profile")
            }
        }
        ShellType::Posix | ShellType::Unknown => {
            // Use .profile for POSIX-compliant shells and unknown shells
            home_dir.join(".profile")
        }
        ShellType::Cmd => {
            // CMD shouldn't be handled by Unix setup, but provide a fallback
            home_dir.join(".profile")
        }
    }
}

/// Generate Unix environment file with proper escaping and shell-specific content
pub async fn generate_unix_env_file(
    shell: &Shell,
    env_file_path: &Path,
    zv_dir: &Path,
    bin_path: &Path,
    export_zv_dir: bool,
) -> crate::Result<()> {
    use crate::shell::path_utils::{escape_path_for_shell, normalize_path_for_shell};

    // Normalize and escape paths for the shell
    let zv_dir_str = normalize_path_for_shell(shell, zv_dir);
    let bin_path_str = normalize_path_for_shell(shell, bin_path);
    let escaped_zv_dir = escape_path_for_shell(shell, &zv_dir_str);
    let escaped_bin_path = escape_path_for_shell(shell, &bin_path_str);

    // Generate shell-specific content
    let content = shell.generate_env_content(&escaped_zv_dir, &escaped_bin_path, export_zv_dir);

    // Create parent directories if they don't exist
    if let Some(parent) = env_file_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|_| {
            crate::ZvError::shell_environment_file_failed(
                "create_directory",
                &env_file_path.display().to_string(),
            )
        })?;
    }

    // Write the environment file with proper line endings
    crate::shell::env_export::write_shell_file_with_line_endings(env_file_path, &content)
        .await
        .map_err(|e| {
            tracing::error!(target: TARGET,
                "Failed to write environment file {}: {}",
                env_file_path.display(),
                e
            );
            crate::ZvError::shell_environment_file_failed(
                "write",
                &env_file_path.display().to_string(),
            )
        })?;

    Ok(())
}

/// Add source line to RC file with proper shell-specific syntax
pub async fn add_source_to_rc_file(
    shell: &Shell,
    rc_file: &Path,
    env_file_path: &Path,
) -> crate::Result<()> {
    // Generate shell-specific source command
    let source_line = shell.get_source_command(env_file_path);

    // Read existing content or create empty content
    let mut content = if rc_file.exists() {
        tokio::fs::read_to_string(rc_file).await.map_err(|e| {
            crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
        })?
    } else {
        String::new()
    };

    // Check if source line already exists
    if content
        .lines()
        .any(|line| line.trim() == source_line.trim())
    {
        return Ok(()); // Already exists, no need to add
    }

    // Add source line with comment
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("# Added by zv setup\n");
    content.push_str(&source_line);
    content.push('\n');

    // Create parent directories if needed
    if let Some(parent) = rc_file.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
        })?;
    }

    // Write the updated content with proper line endings
    write_rc_file_with_line_endings(rc_file, &content).await.map_err(|e| {
        crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
    })?;

    Ok(())
}

/// Add ZV_DIR export to RC file with proper shell-specific syntax
pub async fn add_zv_dir_export_to_rc_file(
    shell: &Shell,
    rc_file: &Path,
    zv_dir: &Path,
) -> crate::Result<()> {
    use crate::shell::path_utils::{escape_path_for_shell, normalize_path_for_shell};

    // Normalize and escape the ZV_DIR path
    let zv_dir_str = normalize_path_for_shell(shell, zv_dir);
    let escaped_zv_dir = escape_path_for_shell(shell, &zv_dir_str);

    // Generate shell-specific export command
    let export_line = match shell.shell_type {
        ShellType::Fish => {
            format!("set -gx ZV_DIR {}", escaped_zv_dir)
        }
        ShellType::Tcsh => {
            format!("setenv ZV_DIR {}", escaped_zv_dir)
        }
        ShellType::Nu => {
            format!("$env.ZV_DIR = {}", escaped_zv_dir)
        }
        _ => {
            // POSIX-compliant shells (bash, zsh, etc.)
            format!("export ZV_DIR={}", escaped_zv_dir)
        }
    };

    // Read existing content or create empty content
    let mut content = if rc_file.exists() {
        tokio::fs::read_to_string(rc_file).await.map_err(|e| {
            crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
        })?
    } else {
        String::new()
    };

    // Check if ZV_DIR export already exists (look for any ZV_DIR setting)
    let has_zv_dir_export = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("export ZV_DIR=")
            || trimmed.starts_with("set -gx ZV_DIR ")
            || trimmed.starts_with("setenv ZV_DIR ")
            || trimmed.starts_with("$env.ZV_DIR =")
    });

    if has_zv_dir_export {
        return Ok(()); // Already exists, no need to add
    }

    // Add export line with comment
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("# Added by zv setup\n");
    content.push_str(&export_line);
    content.push('\n');

    // Create parent directories if needed
    if let Some(parent) = rc_file.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
        })?;
    }

    // Write the updated content with proper line endings
    write_rc_file_with_line_endings(rc_file, &content).await.map_err(|e| {
        crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
    })?;

    Ok(())
}

/// Check if ZV_DIR is permanently set in Unix environment
pub async fn check_zv_dir_permanent_unix(shell: &Shell, zv_dir: &Path) -> crate::Result<bool> {
    let rc_file = select_rc_file(shell);

    if !rc_file.exists() {
        return Ok(false);
    }

    let content = tokio::fs::read_to_string(&rc_file).await.map_err(|e| {
        crate::ZvError::shell_rc_file_modification_failed(&rc_file.display().to_string(), e)
    })?;

    // Normalize the target path for comparison
    let target_path_str = crate::shell::path_utils::normalize_path_for_shell(shell, zv_dir);

    // Check if any ZV_DIR export exists that matches our target path
    let has_matching_export = content.lines().any(|line| {
        let trimmed = line.trim();

        // Skip comments
        if trimmed.starts_with('#') {
            return false;
        }

        // Extract the path from different shell export formats
        let exported_path = if let Some(path) = trimmed.strip_prefix("export ZV_DIR=") {
            Some(path.trim_matches('"').trim_matches('\''))
        } else if let Some(path) = trimmed.strip_prefix("set -gx ZV_DIR ") {
            Some(path.trim_matches('"').trim_matches('\''))
        } else if let Some(path) = trimmed.strip_prefix("setenv ZV_DIR ") {
            Some(path.trim_matches('"').trim_matches('\''))
        } else { trimmed.strip_prefix("$env.ZV_DIR = ").map(|path| path.trim_matches('"').trim_matches('\'')) };

        if let Some(path) = exported_path {
            // Compare normalized paths
            let normalized_exported = if path.starts_with('~') {
                // Expand tilde to home directory for comparison
                if let Some(home) = dirs::home_dir() {
                    path.replacen('~', &home.to_string_lossy(), 1)
                } else {
                    path.to_string()
                }
            } else {
                path.to_string()
            };

            // Normalize the exported path for comparison
            let normalized_exported = Path::new(&normalized_exported);
            let normalized_exported_str =
                crate::shell::path_utils::normalize_path_for_shell(shell, normalized_exported);

            normalized_exported_str == target_path_str
        } else {
            false
        }
    });

    Ok(has_matching_export)
}

/// Execute ZV_DIR setup for Unix systems
pub async fn execute_zv_dir_setup_unix(
    context: &crate::shell::setup::SetupContext,
    zv_dir: &Path,
) -> crate::Result<()> {
    // Validate that the ZV_DIR path exists or can be created
    if !zv_dir.exists() {
        // Try to create the directory to validate the path
        if let Err(e) = tokio::fs::create_dir_all(zv_dir).await {
            return Err(crate::types::error::ShellErr::ZvDirOperationFailed {
                operation: format!(
                    "ZV_DIR setup failed: cannot create directory {}: {}",
                    zv_dir.display(),
                    e
                ),
            }
            .into());
        }
    }

    let rc_file = select_rc_file(&context.shell);

    add_zv_dir_export_to_rc_file(&context.shell, &rc_file, zv_dir).await?;

    // Track the RC file modification
    use crate::shell::setup::instructions::{FileAction, create_rc_file_entry};
    context.add_modified_file(create_rc_file_entry(rc_file.clone(), FileAction::Modified));

    println!(
        "✓ Added ZV_DIR export to {}",
        Paint::green(&rc_file.display().to_string())
    );

    Ok(())
}

/// Execute PATH setup for Unix systems
pub async fn execute_path_setup_unix(
    context: &crate::shell::setup::SetupContext,
    env_file_path: &Path,
    rc_file: &Path,
    bin_path: &Path,
) -> crate::Result<()> {
    use crate::shell::setup::instructions::{
        FileAction, create_env_file_entry, create_rc_file_entry,
    };

    // Generate the environment file
    generate_unix_env_file(&context.shell, env_file_path, context.app.path(), bin_path, context.using_env_var).await?;

    println!(
        "✓ Generated environment file at {}",
        Paint::green(&env_file_path.display().to_string())
    );

    // Track the environment file creation
    context.add_modified_file(create_env_file_entry(
        env_file_path.to_path_buf(),
        FileAction::Created,
    ));

    // Add source line to RC file
    add_source_to_rc_file(&context.shell, rc_file, env_file_path).await?;

    println!(
        "✓ Added source line to {}",
        Paint::green(&rc_file.display().to_string())
    );

    // Track the RC file modification
    context.add_modified_file(create_rc_file_entry(
        rc_file.to_path_buf(),
        FileAction::SourceAdded,
    ));

    Ok(())
}
/// Write RC file content with proper line endings (always Unix LF for RC files)
async fn write_rc_file_with_line_endings(file_path: &Path, content: &str) -> Result<(), std::io::Error> {
    // RC files should always use Unix line endings (LF) even on Windows
    // because they're shell configuration files
    let normalized_content = content.replace("\r\n", "\n");
    tokio::fs::write(file_path, normalized_content).await
}