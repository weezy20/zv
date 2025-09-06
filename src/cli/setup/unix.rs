use color_eyre::eyre::{Context as _, eyre};
use std::path::{Path, PathBuf};
use yansi::Paint;

use crate::{App, Shell, suggest};

pub async fn setup_unix_environment(
    app: &mut App,
    shell: &Shell,
    using_env_var: bool,
    dry_run: bool,
) -> crate::Result<()> {
    let zv_dir = app.path();

    // Generate shell environment file
    let (env_file, env_content) = shell.export_without_dump(app, using_env_var);

    // Check if environment file needs to be created/updated
    let env_file_needs_update = if env_file.exists() {
        match tokio::fs::read_to_string(&env_file).await {
            Ok(existing_content) => existing_content.trim() != env_content.trim(),
            Err(_) => true,
        }
    } else {
        true
    };

    // Check if shell RC files need to be updated
    let rc_files_need_update = !check_shell_rc_files_configured(shell, zv_dir).await;

    // If no updates are needed, inform the user
    if !env_file_needs_update && !rc_files_need_update {
        println!(
            "{}",
            Paint::green("✓ Unix shell environment is already configured correctly")
        );
        println!(
            "  • Environment file: {} (up to date)",
            Paint::dim(&env_file.display().to_string())
        );
        println!(
            "  • Shell startup files: {} (already configured)",
            Paint::dim("no changes needed")
        );
        return Ok(());
    }

    // Show what will be written to the environment file
    if env_file_needs_update {
        if dry_run {
            println!(
                "{} shell environment file: {}",
                Paint::yellow("Would create/update"),
                Paint::cyan(&env_file.display().to_string())
            );
        } else {
            println!(
                "Creating/updating shell environment file: {}",
                Paint::cyan(&env_file.display().to_string())
            );
        }

        println!("\nEnvironment file contents:");
        println!("{}", Paint::dim(&"─".repeat(50)));
        for line in env_content.lines() {
            if line.trim().starts_with('#') {
                println!("{}", Paint::dim(line));
            } else if line.contains("export") || line.contains("set") || line.contains("setenv") {
                println!("{}", Paint::green(line));
            } else {
                println!("{}", line);
            }
        }
        println!("{}", Paint::dim(&"─".repeat(50)));
        println!();
    } else {
        println!(
            "Environment file: {} (already up to date)",
            Paint::dim(&env_file.display().to_string())
        );
    }

    if !dry_run && env_file_needs_update {
        // Write the environment file
        shell
            .export(app, using_env_var)
            .await
            .map_err(|e| eyre!("Failed to create environment file: {}", e))?;

        println!("{}", Paint::green("✓ Generated shell environment file"));
    }

    // Show which RC files will be checked/modified
    let rc_files = shell.get_rc_files();
    if !rc_files.is_empty() && rc_files_need_update {
        let action = if dry_run { "Would check" } else { "Checking" };
        println!("\n{} shell startup files for {} shell:", action, shell);
        for file in &rc_files {
            let exists = file.exists();
            let status = if exists { "exists" } else { "will be created" };
            println!(
                "  • {} ({})",
                Paint::dim(&file.display().to_string()),
                Paint::yellow(status)
            );
        }
        println!();
    } else if !rc_files_need_update {
        println!(
            "\nShell startup files: {} (already configured)",
            Paint::dim("no changes needed")
        );
    }

    // Add sourcing to shell startup files
    let source_command = shell.get_source_command(&env_file);

    if dry_run {
        if rc_files_need_update {
            // Preview what would be added to RC files
            println!("{} to shell startup files:", Paint::yellow("Would add"));
            println!("  {}", Paint::dim("# Added by zv setup"));
            println!("  {}", Paint::green(&source_command));
            println!();
        }

        println!("{}", Paint::yellow("Dry run - no changes were made"));
        println!("Run {} to apply these changes", Paint::green("zv setup"));
    } else {
        if rc_files_need_update {
            let modified_files = add_source_to_shell_files(shell, &env_file).await?;

            println!("{}", Paint::green("✓ Shell setup complete"));

            // Show what was actually modified
            if !modified_files.is_empty() {
                println!("\nModified shell startup files:");
                for file in &modified_files {
                    println!(
                        "  • {} (added: {})",
                        Paint::green(&file.display().to_string()),
                        Paint::dim(&format!("# Added by zv setup\\n{}", source_command))
                    );
                }
            } else {
                println!(
                    "\n{}",
                    Paint::yellow(
                        "No shell startup files were modified (source line already exists)"
                    )
                );
            }
        } else {
            println!(
                "{}",
                Paint::green("✓ Shell setup complete (no RC file changes needed)")
            );
        }

        suggest!(
            "Restart your shell or run {} to apply changes immediately",
            cmd = &format!("source {}", env_file.display())
        );
    }

    Ok(())
}

async fn check_shell_rc_files_configured(shell: &Shell, zv_dir: &Path) -> bool {
    let rc_files = shell.get_rc_files();
    let env_file = zv_dir.join(shell.env_file_name());
    let expected_source = shell.get_source_command(&env_file);

    // Check if any RC file contains the source command
    for rc_file in rc_files {
        if rc_file.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&rc_file).await {
                // Check if the file contains a source command for our env file
                let has_source = content.lines().any(|line| {
                    let trimmed = line.trim();
                    trimmed == expected_source.trim()
                        || (trimmed.starts_with("source")
                            && trimmed.contains(&env_file.to_string_lossy().as_ref()))
                });

                if has_source {
                    return true;
                }
            }
        }
    }

    false
}

pub async fn add_source_to_shell_files(
    shell: &Shell,
    env_file: &Path,
) -> crate::Result<Vec<PathBuf>> {
    let home_dir = dirs::home_dir().ok_or_else(|| eyre!("Could not determine home directory"))?;

    // Generate appropriate source command for the shell
    let source_line = shell.get_source_command(env_file);

    // Get shell-specific RC files
    let shell_files = shell.get_rc_files();

    let mut modified_files = Vec::new();

    for shell_file in shell_files {
        match add_source_to_file(&shell_file, &source_line).await {
            Ok(was_modified) => {
                if was_modified {
                    modified_files.push(shell_file);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to modify {}: {}", shell_file.display(), e);
                // If we can't write to shell-specific file, try .profile as fallback
                if shell_file != home_dir.join(".profile") {
                    if let Ok(was_modified) =
                        add_source_to_file(&home_dir.join(".profile"), &source_line).await
                    {
                        if was_modified {
                            modified_files.push(home_dir.join(".profile"));
                        }
                    }
                }
            }
        }
    }

    Ok(modified_files)
}

pub async fn add_source_to_file(file_path: &Path, source_line: &str) -> crate::Result<bool> {
    use tokio::fs::{OpenOptions, metadata};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // Check if file exists and read content
    let mut content = String::new();
    let file_exists = if let Ok(_) = metadata(file_path).await {
        let mut file = tokio::fs::File::open(file_path)
            .await
            .with_context(|| format!("Failed to open {}", file_path.display()))?;
        file.read_to_string(&mut content)
            .await
            .with_context(|| format!("Failed to read {}", file_path.display()))?;
        true
    } else {
        false
    };

    // Check if source line already exists (check both the exact line and just the source command)
    let source_exists = content.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == source_line.trim()
            || (trimmed.starts_with("source")
                && trimmed.contains(&source_line.trim().replace("source ", "")))
    });

    if source_exists {
        tracing::debug!("Source line already exists in {}", file_path.display());
        return Ok(false); // File was not modified
    }

    // Create parent directories if they don't exist
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }

    // Prepare the content to add
    let addition = format!("# Added by zv setup\n{}\n", source_line);

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

    file.write_all(addition.as_bytes())
        .await
        .with_context(|| format!("Failed to write to {}", file_path.display()))?;

    tracing::info!("Added zv setup to {}", file_path.display());

    Ok(true) // File was modified
}
