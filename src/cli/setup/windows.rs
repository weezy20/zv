use cfg_if::cfg_if;
use color_eyre::eyre::eyre;
use yansi::Paint;

use crate::App;

cfg_if! {
    if #[cfg(target_os = "windows")] {
        pub async fn setup_windows_environment(
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
    } else {
        pub async fn setup_windows_environment(
            _app: &App,
            _using_env_var: bool,
            _dry_run: bool,
        ) -> crate::Result<()> {
            unreachable!("Windows setup should not be called on non-Windows platforms")
        }
    }
}
