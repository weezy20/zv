use color_eyre::eyre::eyre;
use yansi::Paint;

use crate::{App, shell::path_utils::*};
use super::setup_utils::{SetupRequirements, set_zv_dir_env_var};

#[cfg(target_os = "windows")]
pub async fn setup_windows_environment(
    app: &App,
    requirements: &SetupRequirements,
    dry_run: bool,
) -> crate::Result<()> {
    use windows_registry::{CURRENT_USER, Value};

    let zv_dir = app.path();
    let bin_path = app.bin_path();
    
    // Use shell-aware path formatting
    let shell = app.shell().unwrap_or(&crate::shell::Shell::Cmd);
    let zv_dir_str = normalize_path_for_shell(shell, zv_dir);
    let bin_path_str = normalize_path_for_shell(shell, bin_path);

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

    // ZV_DIR handling
    let zv_dir_needs_update = requirements.set_zv_dir_env && current_zv_dir.is_none();

    let path_already_contains_bin = current_path.split(';').any(|p| p.trim() == bin_path_str);
    let path_needs_update = !path_already_contains_bin;

    // If no changes are needed, inform the user
    if !path_needs_update && !zv_dir_needs_update {
        println!(
            "{}",
            Paint::green("✓ Windows environment variables are already configured correctly")
        );
        if requirements.set_zv_dir_env {
            println!(
                "  • ZV_DIR: {} ({})",
                Paint::green(&zv_dir_str),
                if current_zv_dir.is_some() { "already set" } else { "using environment variable" }
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

    // Set ZV_DIR environment variable if requested
    if requirements.set_zv_dir_env {
        set_zv_dir_env_var(app, shell, dry_run).await?;
        println!();
    }

    // Show what will be changed
    println!("\nRegistry changes to be made:");

    // ZV_DIR changes
    if zv_dir_needs_update {
        println!("  ZV_DIR: setting to {}", Paint::green(&zv_dir_str));
    } else if requirements.set_zv_dir_env {
        println!(
            "  ZV_DIR: {} (already set)",
            Paint::dim(&current_zv_dir.as_deref().unwrap_or("using environment variable"))
        );
    } else {
        println!(
            "  ZV_DIR: {} (using default path)",
            Paint::dim("not setting in registry")
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

#[cfg(not(target_os = "windows"))]
pub async fn setup_windows_environment(
    _app: &App,
    _requirements: &SetupRequirements,
    _dry_run: bool,
) -> crate::Result<()> {
    unreachable!("Windows setup should not be called on non-Windows platforms")
}
