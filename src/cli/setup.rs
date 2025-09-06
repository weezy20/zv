use color_eyre::eyre::{Context as _, eyre};
use std::fs::File;
use std::io::Read;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use yansi::Paint;

use crate::tools::{calculate_file_hash, canonicalize, files_have_same_hash};
use crate::{App, Shell, ZigVersion, suggest, tools};

/// Check if we're using a custom ZV_DIR (not the default $HOME/.zv) and warn the user
fn check_custom_zv_dir_warning(app: &App, using_env_var: bool) -> crate::Result<bool> {
    if !using_env_var {
        // Using default path, no warning needed
        return Ok(true);
    }

    let zv_dir = app.path();
    let home_dir = dirs::home_dir().ok_or_else(|| eyre!("Could not determine home directory"))?;
    let default_zv_dir = home_dir.join(".zv");

    // Show warning about custom ZV_DIR
    println!("{}", Paint::yellow("⚠ Custom ZV_DIR Warning").bold());
    println!();
    println!(
        "You are using a custom ZV_DIR path: {}",
        Paint::cyan(&zv_dir.display().to_string())
    );
    println!(
        "Default path would be: {}",
        Paint::dim(&default_zv_dir.display().to_string())
    );
    println!();
    println!("{}", Paint::yellow("Important considerations:"));
    println!(
        "• ZV_DIR must be {} set in your environment",
        Paint::red("permanently")
    );
    println!(
        "• Temporary ZV_DIR settings will break zv in new sessions unless the next session also has it set"
    );
    println!("• Ensure ZV_DIR is permanently set in your shell profile or system environment");
    println!("• If yes you can ignore this warning");
    println!();

    // Prompt for confirmation
    print!("Do you want to continue with the custom ZV_DIR path? [y/N]: ");
    io::stdout()
        .flush()
        .map_err(|e| eyre!("Failed to flush stdout: {}", e))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| eyre!("Failed to read user input: {}", e))?;

    let response = input.trim().to_lowercase();
    let should_continue = matches!(response.as_str(), "y" | "yes");

    if !should_continue {
        println!();
        println!("{}", Paint::yellow("Setup aborted by user."));
        println!();
        println!("To use the default ZV_DIR path, unset the ZV_DIR environment variable:");
        if cfg!(windows) {
            println!("  {}", Paint::green("Remove-Item Env:ZV_DIR"));
        } else {
            println!("  {}", Paint::green("unset ZV_DIR"));
        }
        println!("Then run {} again.", Paint::green("zv setup"));
        return Ok(false);
    }

    println!();
    println!("{}", Paint::green("Continuing with custom ZV_DIR path..."));
    println!();

    Ok(true)
}

/// Check if setup is needed by verifying if zv bin path is already in PATH
/// and if shell environment is properly configured
pub async fn pre_setup_checks(
    app: &App,
    shell: &Shell,
    using_env_var: bool,
) -> crate::Result<bool> {
    let zv_dir = app.path();
    let bin_path = app.bin_path();

    // Check if bin path is already in system PATH - this is the most important check
    let path_already_in_system = shell.check_path_in_system(&bin_path);

    // Check if the zv binary in bin path is up to date (hash comparison)
    let current_exe = std::env::current_exe()
        .map_err(|e| eyre!("Failed to get current executable path: {}", e))?;

    let target_exe = if cfg!(windows) {
        app.bin_path().join("zv.exe")
    } else {
        app.bin_path().join("zv")
    };

    let binary_up_to_date = if target_exe.exists() {
        match files_have_same_hash(&current_exe, &target_exe) {
            Ok(same) => same,
            Err(_) => false, // If we can't compare, assume it needs updating
        }
    } else {
        false // Binary doesn't exist, needs copying
    };

    // If bin path is already in PATH and binary is up to date, we're essentially done
    if path_already_in_system && binary_up_to_date {
        println!("{}", Paint::green("✓ zv is already configured"));
        println!(
            "  • {} is already in PATH",
            Paint::green(&bin_path.display().to_string())
        );
        println!(
            "  • {} is up to date",
            Paint::green(&target_exe.display().to_string())
        );
        println!();

        // Still check if shims need regeneration
        println!("Checking if shim regeneration is needed...");
        return Ok(false); // No setup needed, but post-setup will be checked separately
    }

    // Check for custom ZV_DIR and warn user if needed (only if setup is actually needed)
    if !check_custom_zv_dir_warning(app, using_env_var)? {
        return Ok(false); // User chose to abort
    }

    // Check if ZV_DIR environment variable is set correctly (for informational purposes)
    let zv_dir_set = if using_env_var {
        // When using custom ZV_DIR, verify it matches what we expect
        match std::env::var("ZV_DIR") {
            Ok(env_zv_dir) => {
                let env_path = PathBuf::from(env_zv_dir);
                match (canonicalize(&env_path), canonicalize(&zv_dir)) {
                    (Ok(env_canonical), Ok(zv_canonical)) => env_canonical == zv_canonical,
                    _ => false,
                }
            }
            Err(_) => false,
        }
    } else {
        // When using default path, ZV_DIR should not be set (or we don't care)
        true
    };

    // For Unix systems, check if shell RC files contain the source command
    let shell_rc_configured = if cfg!(windows) {
        // On Windows, we only need PATH to be set
        true
    } else {
        check_shell_rc_files_configured(shell, zv_dir).await
    };

    // Show what needs to be configured
    println!("Setup status check:");
    println!(
        "  ✗ {} is not in PATH",
        Paint::red(&bin_path.display().to_string())
    );

    if binary_up_to_date {
        println!(
            "  ✓ {} is up to date",
            Paint::green(&target_exe.display().to_string())
        );
    } else if target_exe.exists() {
        println!(
            "  ✗ {} exists but is outdated (hash mismatch)",
            Paint::red(&target_exe.display().to_string())
        );
    } else {
        println!(
            "  ✗ {} does not exist",
            Paint::red(&target_exe.display().to_string())
        );
    }

    if using_env_var {
        if zv_dir_set {
            println!("  ✓ ZV_DIR environment variable is set correctly");
        } else {
            println!("  ✗ ZV_DIR environment variable mismatch");
        }
    } else {
        println!(
            "  • ZV_DIR: {} (using default path)",
            Paint::dim("not needed")
        );
    }

    if cfg!(unix) {
        if shell_rc_configured {
            println!("  ✓ Shell startup files are configured");
        } else {
            println!("  ✗ Shell startup files need configuration");
        }
    }

    println!();

    Ok(true) // Setup is needed
}

/// Check if shell RC files are already configured with zv setup
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

/// Copy the current zv binary to the bin directory if needed
async fn copy_zv_binary_if_needed(app: &App, dry_run: bool) -> crate::Result<()> {
    let current_exe = std::env::current_exe()
        .map_err(|e| eyre!("Failed to get current executable path: {}", e))?;

    let target_exe = if cfg!(windows) {
        app.bin_path().join("zv.exe")
    } else {
        app.bin_path().join("zv")
    };

    // Check if target exists and compare hashes
    if target_exe.exists() {
        match files_have_same_hash(&current_exe, &target_exe) {
            Ok(true) => {
                println!("  ✓ zv binary is up to date in bin directory");
                return Ok(());
            }
            Ok(false) => {
                if dry_run {
                    println!(
                        "  {} zv binary in bin directory (hash mismatch)",
                        Paint::yellow("Would update")
                    );
                } else {
                    println!("  • Updating zv binary in bin directory (hash mismatch)");
                }
            }
            Err(e) => {
                println!(
                    "  • {} hash comparison: {}, will copy anyway",
                    Paint::yellow("Warning"),
                    e
                );
            }
        }
    } else {
        if dry_run {
            println!(
                "  {} zv binary to bin directory",
                Paint::yellow("Would copy")
            );
        } else {
            println!("  • Copying zv binary to bin directory");
        }
    }

    if !dry_run {
        // Create bin directory if it doesn't exist
        tokio::fs::create_dir_all(app.bin_path())
            .await
            .map_err(|e| eyre!("Failed to create bin directory: {}", e))?;

        // Copy the current executable to the target location
        tokio::fs::copy(&current_exe, &target_exe)
            .await
            .map_err(|e| eyre!("Failed to copy zv binary to bin directory: {}", e))?;

        println!(
            "    {} Copied {} to {}",
            Paint::green("✓"),
            current_exe.display(),
            target_exe.display()
        );
    }

    Ok(())
}

/// Regenerate hardlinks/shims for zig and zls if they exist and config is available
async fn regenerate_shims_if_needed(app: &App, dry_run: bool) -> crate::Result<()> {
    let zig_shim = if cfg!(windows) {
        app.bin_path().join("zig.exe")
    } else {
        app.bin_path().join("zig")
    };

    let zls_shim = if cfg!(windows) {
        app.bin_path().join("zls.exe")
    } else {
        app.bin_path().join("zls")
    };

    let has_zig_shim = zig_shim.exists();
    let has_zls_shim = zls_shim.exists();

    if !has_zig_shim && !has_zls_shim {
        println!("  • No zig/zls shims found - nothing to regenerate");
        return Ok(());
    }

    // Check if config.toml exists
    let config_path = app.path().join("config.toml");
    if !config_path.exists() {
        if has_zig_shim || has_zls_shim {
            println!(
                "  {} config.toml not found - cannot regenerate shims",
                Paint::yellow("⚠")
            );
            println!("    Consider running 'zv use <version>' to set up configuration");
        }
        return Ok(());
    }

    if dry_run {
        if has_zig_shim {
            println!(
                "  {} zig shim based on config.toml",
                Paint::yellow("Would regenerate")
            );
        }
        if has_zls_shim {
            println!(
                "  {} zls shim based on config.toml",
                Paint::yellow("Would regenerate")
            );
        }
    } else {
        // TODO: Implement actual shim regeneration based on config.toml reading
        // For now, just notify the user
        if has_zig_shim || has_zls_shim {
            println!(
                "  {} Shim regeneration based on config.toml",
                Paint::yellow("TODO")
            );
            println!(
                "    This feature will be implemented to read config.toml and regenerate hardlinks"
            );
            suggest!(
                "Run {} to ensure shims are properly configured",
                cmd = "zv use <version>"
            );
        }
    }

    Ok(())
}

/// Perform post-setup actions: copy zv binary and regenerate shims
async fn post_setup_actions(app: &App, dry_run: bool) -> crate::Result<()> {
    if dry_run {
        println!("\n{} post-setup actions:", Paint::yellow("Would perform"));
    } else {
        println!("\nPerforming post-setup actions:");
    }

    // Copy zv binary to bin directory if needed
    copy_zv_binary_if_needed(app, dry_run).await?;

    // Regenerate shims if needed
    regenerate_shims_if_needed(app, dry_run).await?;

    Ok(())
}

pub async fn setup_shell(
    app: &mut App,
    using_env_var: bool,
    dry_run: bool,
    default_version: ZigVersion,
) -> crate::Result<()> {
    if app.source_set {
        println!("{}", Paint::green("Shell environment already set up."));

        // Even when shell environment is set up, we need to check if binary needs updating
        // or if shims need regeneration
        post_setup_actions(app, dry_run).await?;
        return Ok(());
    }

    let shell = app.shell.unwrap_or_default();

    // Perform pre-setup checks to see if setup is actually needed
    if !dry_run {
        let setup_needed = pre_setup_checks(app, &shell, using_env_var).await?;
        if !setup_needed {
            // Even if setup is not needed, we still need to check post-setup actions
            post_setup_actions(app, dry_run).await?;
            return Ok(());
        }
    }

    if dry_run {
        println!(
            "{} zv setup for {} shell...",
            Paint::yellow("Previewing"),
            Paint::cyan(&shell.to_string())
        );
    } else {
        println!(
            "Setting up zv for {} shell...",
            Paint::cyan(&shell.to_string())
        );
    }

    if cfg!(windows) {
        setup_windows_environment(app, using_env_var, dry_run).await?;
    } else {
        setup_unix_environment(app, &shell, using_env_var, dry_run).await?;
    }

    // Perform post-setup actions: copy zv binary and regenerate shims
    post_setup_actions(app, dry_run).await?;

    Ok(())
}

#[cfg(windows)]
async fn setup_windows_environment(
    app: &App,
    using_env_var: bool,
    dry_run: bool,
) -> crate::Result<()> {
    use windows_registry::{CURRENT_USER, Value};

    let zv_dir = app.path();
    let bin_path = app.bin_path();

    // Set ZV_DIR environment variable
    let zv_dir_str = zv_dir.to_string_lossy();
    let bin_path_str = bin_path.to_string_lossy();

    if dry_run {
        println!(
            "{} Windows environment variables...",
            Paint::yellow("Would set up")
        );
    } else {
        println!("Setting up Windows environment variables...");
    }

    // Open the Environment key for the current user
    let env_key = CURRENT_USER
        .open("Environment")
        .map_err(|e| eyre!("Failed to open Environment registry key: {}", e))?;

    // Get current values to show what's changing
    let current_zv_dir = match env_key.get_string("ZV_DIR") {
        Ok(path) => Some(path),
        _ => None,
    };

    let current_path = match env_key.get_string("PATH") {
        Ok(path) => path,
        Err(_) => String::new(),
    };

    // We should never set ZV_DIR in the Windows registry
    // - Default path: Let the app use $HOME/.zv when ZV_DIR is not set
    // - Custom path: User has already set ZV_DIR environment variable
    let zv_dir_needs_update = false;

    let path_already_contains_bin = current_path.split(';').any(|p| p.trim() == bin_path_str);
    let path_needs_update = !path_already_contains_bin;

    // If no changes are needed, inform the user
    if !path_needs_update {
        println!(
            "{}",
            Paint::green("✓ Windows environment variables are already configured correctly")
        );
        if using_env_var {
            println!(
                "  • ZV_DIR: {} (using environment variable)",
                Paint::dim("custom path")
            );
        } else {
            println!(
                "  • ZV_DIR: {} (using default path)",
                Paint::dim("not set in registry")
            );
        }
        println!(
            "  • PATH: {} (already contains zv bin)",
            Paint::dim("no change needed")
        );
        return Ok(());
    }

    // Show what will be changed
    println!("\nRegistry changes to be made:");

    // ZV_DIR info (we never change it)
    if using_env_var {
        println!(
            "  ZV_DIR: {} (using environment variable, not modifying registry)",
            Paint::dim("custom path")
        );
    } else {
        println!(
            "  ZV_DIR: {} (using default path, not setting in registry)",
            Paint::dim("not needed")
        );
    }

    // PATH changes
    if path_needs_update {
        println!("  PATH: prepending {}", Paint::green(&bin_path_str));
        if !current_path.is_empty() {
            println!(
                "        (to existing PATH with {} entries)",
                current_path.split(';').count()
            );
        }
    } else {
        println!(
            "  PATH: {} (already contains zv bin)",
            Paint::dim("no change needed")
        );
    }

    println!();

    if dry_run {
        println!("{}", Paint::yellow("Dry run - no changes were made"));
        println!("Run {} to apply these changes", Paint::green("zv setup"));
    } else {
        // Apply changes only if needed
        if zv_dir_needs_update {
            env_key
                .set_string("ZV_DIR", &zv_dir_str)
                .map_err(|e| eyre!("Failed to set ZV_DIR environment variable: {}", e))?;
        }

        if path_needs_update {
            let new_path = if current_path.is_empty() {
                bin_path_str.to_string()
            } else {
                format!("{};{}", bin_path_str, current_path)
            };

            env_key
                .set_string("PATH", &new_path)
                .map_err(|e| eyre!("Failed to update PATH environment variable: {}", e))?;
        }

        println!(
            "{}",
            Paint::green("✓ Environment variables set successfully")
        );
        println!(
            "{}",
            Paint::yellow("Please restart your shell or session to apply changes.")
        );
    }

    Ok(())
}

#[cfg(not(windows))]
async fn setup_windows_environment(
    _app: &App,
    _using_env_var: bool,
    _dry_run: bool,
) -> crate::Result<()> {
    unreachable!("Windows setup should not be called on non-Windows platforms")
}

async fn setup_unix_environment(
    app: &App,
    shell: &Shell,
    using_env_var: bool,
    dry_run: bool,
) -> crate::Result<()> {
    let zv_dir = app.path();

    // Generate shell environment file
    let (env_file, env_content) = shell.export_without_dump(zv_dir, app.bin_path(), using_env_var);

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
            .export(zv_dir, app.bin_path(), using_env_var)
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

async fn add_source_to_shell_files(shell: &Shell, env_file: &Path) -> crate::Result<Vec<PathBuf>> {
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

async fn add_source_to_file(file_path: &Path, source_line: &str) -> crate::Result<bool> {
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
