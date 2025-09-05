use yansi::Paint;
use std::path::{Path, PathBuf};
use color_eyre::eyre::{eyre, Context as _};

use crate::{App, Shell, tools, suggest};

pub async fn setup_shell(app: &mut App, using_env_var: bool) -> crate::Result<()> {
    if app.source_set {
        println!(
            "{}",
            Paint::green("Shell environment already set up. No action needed.")
        );
        return Ok(());
    }

    let shell = app.shell.unwrap_or_default();
    
    println!("Setting up zv for {} shell...", Paint::cyan(&shell.to_string()));
    
    if cfg!(windows) {
        setup_windows_environment(app).await?;
    } else {
        setup_unix_environment(app, &shell, using_env_var).await?;
    }
    
    Ok(())
}

#[cfg(windows)]
async fn setup_windows_environment(app: &App) -> crate::Result<()> {
    use windows_registry::{CURRENT_USER, Value};
    
    let zv_dir = app.path();
    let bin_path = zv_dir.join("bin");
    
    // Set ZV_DIR environment variable
    let zv_dir_str = zv_dir.to_string_lossy();
    
    println!("Setting up Windows environment variables...");
    
    // Open the Environment key for the current user
    let env_key = CURRENT_USER
        .open("Environment")
        .map_err(|e| eyre!("Failed to open Environment registry key: {}", e))?;
    
    // Set ZV_DIR
    env_key
        .set_value("ZV_DIR", &Value::String(zv_dir_str.to_string()))
        .map_err(|e| eyre!("Failed to set ZV_DIR environment variable: {}", e))?;
    
    // Get current PATH
    let current_path = match env_key.get_value("PATH") {
        Ok(Value::String(path)) => path,
        Ok(_) => String::new(),
        Err(_) => String::new(),
    };
    
    let bin_path_str = bin_path.to_string_lossy();
    
    // Check if bin path is already in PATH
    if !current_path.split(';').any(|p| p.trim() == bin_path_str) {
        let new_path = if current_path.is_empty() {
            bin_path_str.to_string()
        } else {
            format!("{};{}", bin_path_str, current_path)
        };
        
        env_key
            .set_value("PATH", &Value::String(new_path))
            .map_err(|e| eyre!("Failed to update PATH environment variable: {}", e))?;
    }
    
    println!("{}", Paint::green("✓ Environment variables set successfully"));
    println!("{}", Paint::yellow("Please restart your shell or session to apply changes."));
    
    Ok(())
}

#[cfg(not(windows))]
async fn setup_windows_environment(_app: &App) -> crate::Result<()> {
    unreachable!("Windows setup should not be called on non-Windows platforms")
}

async fn setup_unix_environment(app: &App, shell: &Shell, using_env_var: bool) -> crate::Result<()> {
    let zv_dir = app.path();
    
    // Generate shell environment file
    shell.export(zv_dir, using_env_var).await
        .map_err(|e| eyre!("Failed to create environment file: {}", e))?;
    
    let (env_file, _) = shell.export_without_dump(zv_dir, using_env_var);
    
    println!("{}", Paint::green("✓ Generated shell environment file"));
    
    // Add sourcing to shell startup files
    let modified_files = add_source_to_shell_files(shell, &env_file).await?;
    
    println!("{}", Paint::green("✓ Shell setup complete"));
    
    // Log transparency information
    if !modified_files.is_empty() {
        println!("\nModified files:");
        for file in &modified_files {
            println!("  • {}", Paint::dim(&file.display().to_string()));
        }
    }
    
    suggest!("Restart your shell or run {} to apply changes immediately", cmd = &format!("source {}", env_file.display()));
    
    Ok(())
}

async fn add_source_to_shell_files(shell: &Shell, env_file: &Path) -> crate::Result<Vec<PathBuf>> {
    use tokio::fs::{OpenOptions, metadata};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    let home_dir = dirs::home_dir()
        .ok_or_else(|| eyre!("Could not determine home directory"))?;
    
    // Generate appropriate source command for the shell
    let source_line = match shell {
        Shell::Fish => format!("source \"{}\"", env_file.display()),
        Shell::Nu => format!("source \"{}\"", env_file.display()),
        Shell::Tcsh => format!("source \"{}\"", env_file.display()),
        _ => format!("source \"{}\"", env_file.display()), // POSIX shells (bash, zsh, etc.)
    };
    
    // Files to potentially modify based on shell type
    let shell_files = match shell {
        Shell::Bash => vec![
            home_dir.join(".profile"),
            home_dir.join(".bashrc"),
        ],
        Shell::Zsh => vec![
            home_dir.join(".profile"),
            home_dir.join(".zshrc"),
        ],
        Shell::Fish => vec![
            home_dir.join(".config/fish/config.fish"),
        ],
        Shell::Tcsh => vec![
            home_dir.join(".profile"),
            home_dir.join(".tcshrc"),
        ],
        Shell::Nu => vec![
            home_dir.join(".config/nushell/config.nu"),
        ],
        Shell::Posix | Shell::Unknown => vec![
            home_dir.join(".profile"),
        ],
        _ => vec![
            home_dir.join(".profile"),
        ],
    };
    
    let mut modified_files = Vec::new();
    
    for shell_file in shell_files {
        if let Ok(was_modified) = add_source_to_file(&shell_file, &source_line).await {
            if was_modified {
                modified_files.push(shell_file);
            }
        } else {
            // If we can't write to shell-specific file, try .profile as fallback
            if shell_file != home_dir.join(".profile") {
                if let Ok(was_modified) = add_source_to_file(&home_dir.join(".profile"), &source_line).await {
                    if was_modified {
                        modified_files.push(home_dir.join(".profile"));
                    }
                }
            }
        }
    }
    
    Ok(modified_files)
}

async fn add_source_to_file(file_path: &Path, source_line: &str) -> crate::Result<bool> {
    use tokio::fs::{OpenOptions, metadata};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // Check if file exists and read content
    let mut content = String::new();
    let file_exists = if let Ok(_) = metadata(file_path).await {
        let mut file = tokio::fs::File::open(file_path).await
            .with_context(|| format!("Failed to open {}", file_path.display()))?;
        file.read_to_string(&mut content).await
            .with_context(|| format!("Failed to read {}", file_path.display()))?;
        true
    } else {
        false
    };
    
    // Check if source line already exists
    if content.lines().any(|line| line.trim() == source_line.trim()) {
        return Ok(false); // File was not modified
    }
    
    // Create parent directories if they don't exist
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    
    // Append source line
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(file_path)
        .await
        .with_context(|| format!("Failed to open {} for writing", file_path.display()))?;
    
    // Add newline before if file exists and doesn't end with newline
    if file_exists && !content.is_empty() && !content.ends_with('\n') {
        file.write_all(b"\n").await?;
    }
    
    file.write_all(format!("# Added by zv setup\n{}\n", source_line).as_bytes()).await
        .with_context(|| format!("Failed to write to {}", file_path.display()))?;
    
    println!("Added source line to {}", Paint::dim(&file_path.display().to_string()));
    
    Ok(true) // File was modified
}
