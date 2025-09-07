use crate::{
    App, Shell, path_utils, suggest,
    tools::{canonicalize, files_have_same_hash},
};
use color_eyre::eyre::{Result, eyre};
use dirs;
use std::process::Command;
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};
use yansi::Paint;

/// Setup actions
#[derive(Debug, Clone)]
pub struct SetupRequirements {
    pub set_zv_dir_env: bool,
    pub generate_env_file: bool,
    pub edit_rc_file: bool,
    pub perform_post_setup_action: bool,
}

/// Check if we're using a custom ZV_DIR (not the default $HOME/.zv) and offer to set it permanently
fn check_custom_zv_dir_warning(app: &App, using_env_var: bool, shell: &Shell) -> crate::Result<bool> {
    if !using_env_var {
        // Using default path, no action needed for ZV_DIR
        return Ok(false);
    }

    let zv_dir = app.path();
    let home_dir = dirs::home_dir().ok_or_else(|| eyre!("Could not determine home directory"))?;
    let default_zv_dir = home_dir.join(".zv");

    // Show info about custom ZV_DIR
    println!("{}\n", Paint::yellow("⚠ Custom ZV_DIR detected").bold());
    println!(
        "Your environment has ZV_DIR set to path: {}",
        Paint::cyan(&zv_dir.display().to_string())
    );
    println!(
        "Default path would be: {}",
        Paint::dim(&default_zv_dir.display().to_string())
    );
    println!();

    // Determine the target for permanent setting
    let target_description = if cfg!(windows) {
        "system environment variables"
    } else {
        ".profile"
    };

    // Offer to set ZV_DIR permanently
    print!(
        "Do you want zv to make ZV_DIR={} permanent by adding it to {}? [y/N]: ",
        Paint::cyan(&zv_dir.display().to_string()),
        Paint::green(&target_description)
    );
    io::stdout()
        .flush()
        .map_err(|e| eyre!("Failed to flush stdout: {}", e))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|e| eyre!("Failed to read user input: {}", e))?;

    let response = input.trim().to_lowercase();
    let should_set_permanent = matches!(response.as_str(), "y" | "yes");

    if !should_set_permanent {
        // User chose not to set permanently, show warnings
        println!();
        println!("{}", Paint::yellow("⚠ Important considerations:"));
        println!(
            "• Temporary ZV_DIR settings will break zv in new sessions unless the next session also has it set"
        );
        println!("• Ensure ZV_DIR is permanently set in your shell profile or system environment");
        println!();
        return Ok(false); // Don't set ZV_DIR permanently
    }

    println!();
    println!("{}", Paint::green("zv will set ZV_DIR permanently during setup..."));
    println!();

    Ok(true) // Set ZV_DIR permanently
}

/// Set ZV_DIR environment variable permanently
pub async fn set_zv_dir_env_var(app: &App, shell: &Shell, dry_run: bool) -> crate::Result<()> {
    let zv_dir = app.path();
    
    if dry_run {
        println!(
            "{} ZV_DIR={} permanently",
            Paint::yellow("Would set"),
            Paint::cyan(&zv_dir.display().to_string())
        );
        
        if cfg!(windows) {
            println!("  • Method: Windows registry (system environment variables)");
        } else {
            let rc_files = shell.get_rc_files();
            let preferred_rc = rc_files.first()
                .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
                .unwrap_or_else(|| ".profile".to_string());
            println!("  • Method: Adding export to {}", Paint::cyan(&preferred_rc));
        }
        return Ok(());
    }

    if cfg!(windows) {
        // On Windows, set in system environment variables
        set_windows_env_var("ZV_DIR", &zv_dir.display().to_string())?;
        println!(
            "{} ZV_DIR={}",
            Paint::green("✓ Set"),
            Paint::cyan(&zv_dir.display().to_string())
        );
        println!("  • Location: System environment variables");
    } else {
        // On Unix, add to shell RC file
        set_unix_env_var(shell, zv_dir).await?;
        println!(
            "{} ZV_DIR={}",
            Paint::green("✓ Set"),
            Paint::cyan(&zv_dir.display().to_string())
        );
    }

    Ok(())
}

#[cfg(windows)]
fn set_windows_env_var(var_name: &str, var_value: &str) -> crate::Result<()> {
    use windows_registry::{CURRENT_USER, Value};
    
    let environment_key = CURRENT_USER
        .open("Environment")
        .map_err(|e| eyre!("Failed to open Environment registry key: {}", e))?;
    
    environment_key
        .set_value(var_name, &Value::String(var_value.into()))
        .map_err(|e| eyre!("Failed to set {} environment variable: {}", var_name, e))?;
    
    Ok(())
}

#[cfg(not(windows))]
fn set_windows_env_var(_var_name: &str, _var_value: &str) -> crate::Result<()> {
    unreachable!("Windows environment variable setting should not be called on non-Windows platforms")
}

async fn set_unix_env_var(shell: &Shell, zv_dir: &Path) -> crate::Result<()> {
    let rc_files = shell.get_rc_files();
    let export_line = format!("export ZV_DIR=\"{}\"", zv_dir.display());
    
    // Find the first existing RC file, or create the preferred one
    let target_rc = rc_files.iter()
        .find(|&rc| rc.exists())
        .or_else(|| rc_files.first())
        .ok_or_else(|| eyre!("No suitable RC file found for {} shell", shell))?;

    // Check if ZV_DIR is already set in this file
    if target_rc.exists() {
        let content = tokio::fs::read_to_string(target_rc).await
            .map_err(|e| eyre!("Failed to read {}: {}", target_rc.display(), e))?;
        
        // Check if ZV_DIR export already exists
        let already_has_export = content.lines().any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("export ZV_DIR=") || trimmed.starts_with("ZV_DIR=")
        });
        
        if already_has_export {
            println!("  • ZV_DIR export already exists in {}", 
                     Paint::dim(&target_rc.display().to_string()));
            return Ok(());
        }
    }

    // Add the export line to the RC file
    let mut content = if target_rc.exists() {
        tokio::fs::read_to_string(target_rc).await
            .map_err(|e| eyre!("Failed to read {}: {}", target_rc.display(), e))?
    } else {
        String::new()
    };

    // Add a comment and the export
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("# Added by zv setup for ZV_DIR\n");
    content.push_str(&export_line);
    content.push('\n');

    // Write back to the file
    tokio::fs::write(target_rc, content).await
        .map_err(|e| eyre!("Failed to write to {}: {}", target_rc.display(), e))?;

    println!("  • Added to {}", Paint::cyan(&target_rc.display().to_string()));
    Ok(())
}

/// Check if setup is needed by verifying if zv bin path is already in PATH
/// and if shell environment is properly configured
pub async fn pre_setup_checks(
    app: &App,
    shell: &Shell,
    using_env_var: bool,
) -> crate::Result<Option<SetupRequirements>> {
    let zv_dir = app.path();
    let bin_path = app.bin_path();

    // Check if bin path is already in system PATH - this is the most important check
    let path_already_in_system = path_utils::check_dir_in_path_for_shell(shell, &bin_path);

    // Check if the zv binary in bin path is up to date (hash comparison)
    let current_exe = std::env::current_exe().map_err(|e| {
        eyre!(
            "Pre-setup check: Failed to get current executable path: {}",
            e
        )
    })?;

    let target_exe = if cfg!(windows) {
        app.bin_path().join("zv.exe")
    } else {
        app.bin_path().join("zv")
    };

    let binary_up_to_date = !binary_needs_update(&current_exe, &target_exe);

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
        return Ok(None); // No setup needed, but post-setup will be checked separately
    }

    // Check for custom ZV_DIR and handle environment variable setting
    let set_zv_dir_env = check_custom_zv_dir_warning(app, using_env_var, shell)?;

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
        if set_zv_dir_env {
            println!("  ✓ ZV_DIR environment variable will be set permanently");
        } else {
            println!("  • ZV_DIR environment variable is already set (temporary)");
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

    // Determine what setup actions are needed
    let requirements = SetupRequirements {
        set_zv_dir_env,
        generate_env_file: cfg!(unix), // Unix systems need env files
        edit_rc_file: cfg!(unix) && !shell_rc_configured, // Only edit RC if not configured
        perform_post_setup_action: true, // Always perform post-setup actions
    };

    Ok(Some(requirements)) // Setup is needed
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
pub(super) async fn copy_zv_binary_if_needed(app: &App, dry_run: bool) -> crate::Result<()> {
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
                        "  {} zv binary in bin directory (checksum mismatch)",
                        Paint::yellow("Would update")
                    );
                } else {
                    println!("  • Updating zv binary in bin directory (checksum mismatch)");
                }
            }
            Err(e) => {
                println!(
                    "  • {} checksum comparison: {}, will copy anyway",
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
pub(super) async fn regenerate_shims_if_needed(app: &App, dry_run: bool) -> crate::Result<()> {
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
            suggest!("Run {} to set up configuration", cmd = "zv use <version>");
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

/// Get version from a zv binary executable
fn get_binary_version(exe_path: &Path) -> Result<String> {
    let output = Command::new(exe_path)
        .arg("-V")
        .output()
        .map_err(|e| eyre!("Failed to execute {} -V: {}", exe_path.display(), e))?;

    if !output.status.success() {
        return Err(eyre!("Command {} -V failed", exe_path.display()));
    }

    let version = String::from_utf8(output.stdout)
        .map_err(|e| eyre!("Invalid UTF-8 in version output: {}", e))?
        .trim()
        .to_string();

    Ok(version)
}

/// Compare versions of two executables
fn versions_match(current_exe: &Path, target_exe: &Path) -> Result<bool> {
    Ok(get_binary_version(current_exe)? == get_binary_version(target_exe)?)
}

/// Check if binary needs updating based on version and hash
fn binary_needs_update(current_exe: &Path, target_exe: &Path) -> bool {
    if !target_exe.exists() {
        return true;
    }

    match versions_match(current_exe, target_exe) {
        Ok(true) => {
            // Versions match - use hash as integrity check
            match files_have_same_hash(current_exe, target_exe) {
                Ok(same) => {
                    if !same {
                        println!(
                            "  {} Version match but hash differs - possible corruption",
                            Paint::yellow("⚠")
                        );
                    }
                    !same
                }
                Err(_) => {
                    println!(
                        "  {} Hash check failed but versions match",
                        Paint::yellow("⚠")
                    );
                    false // Assume it's fine if versions match
                }
            }
        }
        Ok(false) => true, // Different versions - needs update
        Err(_) => {
            // Version check failed - fall back to hash comparison
            files_have_same_hash(current_exe, target_exe)
                .map(|same| !same)
                .unwrap_or(true) // If all checks fail, assume update needed
        }
    }
}
