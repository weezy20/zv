//! Sync command and centralized zv binary update functionality
//!
//! This module provides:
//! - `zv sync` command to refresh Zig indices, mirrors, and zv binary
//! - `check_and_update_zv_binary()` - centralized function for updating zv binary
//!   across different commands (sync, setup, use)
//!
//! The binary update logic includes:
//! - Checksum comparison
//! - Version comparison (with optional downgrade prompts)
//! - Automatic shim regeneration when binary is updated

use crate::Shim;
use std::path::Path;

pub async fn sync(app: &mut crate::App) -> crate::Result<()> {
    use yansi::Paint;

    println!("{}", "Syncing Zig indices...".cyan());

    // Force refresh of Zig index from network
    println!("  {} Refreshing Zig index...", "→".blue());
    app.sync_zig_index().await?;
    println!("  {} Zig index synced successfully", "✓".green());

    // Force refresh of mirrors list
    println!("  {} Refreshing community mirrors...", "→".blue());
    let mirror_count = app.sync_mirrors().await?;
    println!(
        "  {} Community mirrors synced successfully ({} mirrors)",
        "✓".green(),
        mirror_count
    );

    // Check and update zv binary if needed
    println!("  {} Checking zv binary...", "→".blue());
    let binary_updated = check_and_update_zv_binary(app, false).await?;

    // Run migrations only if binary was actually updated
    if binary_updated {
        if let Err(e) = crate::app::migrations::migrate(app.path()).await {
            eprintln!("  {} Warning: Migration failed: {}", "⚠".yellow(), e);
        }
    }

    println!("{}", "Sync completed successfully!".green().bold());
    Ok(())
}

/// Public API for checking and updating zv binary
/// This can be called from setup, sync, or other commands
/// Returns true if binary was updated, false if it was already up to date
pub async fn check_and_update_zv_binary(app: &crate::App, quiet: bool) -> crate::Result<bool> {
    tracing::debug!(target: "zv::cli::sync", "Checking for zv binary updates");
    check_and_update_zv_binary_impl(app, quiet, true).await
}

async fn check_and_update_zv_binary_impl(
    app: &crate::App,
    quiet: bool,
    prompt_on_downgrade: bool,
) -> crate::Result<bool> {
    use crate::tools::files_have_same_hash;
    use color_eyre::eyre::Context;

    use yansi::Paint;

    let zv_dir_bin = app.bin_path();
    let target_exe = zv_dir_bin.join(Shim::Zv.executable_name());

    let current_exe = std::env::current_exe().wrap_err("Failed to get current executable path")?;

    // If target doesn't exist, copy current binary
    if !target_exe.exists() {
        if !quiet {
            tracing::info!(
                "zv binary not found in {}, installing...",
                zv_dir_bin.display()
            );
        }
        copy_binary_and_regenerate_shims(&current_exe, &target_exe, app, quiet).await?;
        if !quiet {
            tracing::info!("zv binary installed");
        }
        return Ok(true);
    }

    // Compare checksums
    match files_have_same_hash(&current_exe, &target_exe) {
        Ok(true) => {
            // Checksums match, versions are the same - no update
            if !quiet {
                println!("  {} zv binary is up to date", "✓".green());
            }
            Ok(false)
        }
        Ok(false) => {
            // Checksums differ - need to compare versions
            let current_version = env!("CARGO_PKG_VERSION");

            // Try to get and compare versions
            match get_binary_version(&target_exe) {
                Ok(target_version) => {
                    let current_version = semver::Version::parse(current_version)
                        .expect("CARGO_PKG_VERSION should always be valid semver");

                    use std::cmp::Ordering;
                    match current_version.cmp(&target_version) {
                        Ordering::Greater => {
                            // Current is newer - update target
                            if !quiet {
                                println!(
                                    "  {} Updating zv binary ({} -> {})",
                                    "→".blue(),
                                    Paint::yellow(&target_version),
                                    Paint::green(&current_version)
                                );
                            }
                            copy_binary_and_regenerate_shims(&current_exe, &target_exe, app, quiet)
                                .await?;
                            if !quiet {
                                println!("  {} zv binary updated", "✓".green());
                            }
                            Ok(true)
                        }
                        Ordering::Less => {
                            // Target is newer than current
                            if !quiet {
                                println!(
                                    "  {} Warning: ZV_DIR/bin/zv is newer ({}) than current binary ({})",
                                    "⚠".yellow(),
                                    Paint::green(&target_version),
                                    Paint::yellow(&current_version)
                                );
                            }

                            // Prompt user with default NO (only if prompt_on_downgrade is true)
                            if prompt_on_downgrade && !prompt_user_to_downgrade()? {
                                if !quiet {
                                    println!("  {} Skipping zv binary update", "→".blue());
                                }
                                return Ok(false);
                            }

                            if !quiet {
                                println!(
                                    "  {} {} zv binary ({} -> {})",
                                    "→".blue(),
                                    if prompt_on_downgrade {
                                        "Downgrading"
                                    } else {
                                        "Updating"
                                    },
                                    Paint::green(&target_version),
                                    Paint::yellow(&current_version)
                                );
                            }
                            copy_binary_and_regenerate_shims(&current_exe, &target_exe, app, quiet)
                                .await?;
                            if !quiet {
                                println!(
                                    "  {} zv binary {}",
                                    "✓".green(),
                                    if prompt_on_downgrade {
                                        "downgraded"
                                    } else {
                                        "updated"
                                    }
                                );
                            }
                            Ok(true)
                        }
                        Ordering::Equal => {
                            // Same version but different checksum - update
                            if !quiet {
                                println!(
                                    "  {} Updating zv binary (checksum mismatch for version {})",
                                    "→".blue(),
                                    current_version
                                );
                            }
                            copy_binary_and_regenerate_shims(&current_exe, &target_exe, app, quiet)
                                .await?;
                            if !quiet {
                                println!("  {} zv binary updated", "✓".green());
                            }
                            Ok(true)
                        }
                    }
                }
                Err(e) => {
                    // Failed to get version - assume we need to replace
                    tracing::error!(
                        target: "zv::cli::sync",
                        error = %e,
                        "Failed to get version from target binary, will update anyway"
                    );
                    if !quiet {
                        println!(
                            "  {} Warning: failed to get target version, updating anyway",
                            "⚠".yellow()
                        );
                    }
                    copy_binary_and_regenerate_shims(&current_exe, &target_exe, app, quiet).await?;
                    if !quiet {
                        println!("  {} zv binary updated", "✓".green());
                    }
                    Ok(true)
                }
            }
        }
        Err(e) => {
            // Checksum comparison failed - update anyway
            if !quiet {
                println!(
                    "  {} Warning: checksum comparison failed: {}, updating anyway",
                    "⚠".yellow(),
                    e
                );
            }
            copy_binary_and_regenerate_shims(&current_exe, &target_exe, app, quiet).await?;
            if !quiet {
                println!("  {} zv binary updated", "✓".green());
            }
            Ok(true)
        }
    }
}

/// Get version from a zv binary by running it with --version
fn get_binary_version(binary_path: &std::path::Path) -> crate::Result<semver::Version> {
    use color_eyre::eyre::eyre;

    let output = std::process::Command::new(binary_path)
        .arg("--version")
        .output()
        .map_err(|e| {
            eyre!(
                "Failed to execute binary at {}: {}",
                binary_path.display(),
                e
            )
        })?;

    if !output.status.success() {
        return Err(eyre!(
            "Binary at {} failed to run --version",
            binary_path.display()
        ));
    }

    let version_output = String::from_utf8_lossy(&output.stdout);
    // Parse "zv X.Y.Z" format - extract version number
    let version_str = version_output
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| eyre!("Failed to parse version from: {}", version_output))?
        .trim();

    // Parse as semver
    semver::Version::parse(version_str)
        .map_err(|e| eyre!("Failed to parse version '{}' as semver: {}", version_str, e))
}

/// Prompt user whether to proceed with downgrade (default: NO)
fn prompt_user_to_downgrade() -> crate::Result<bool> {
    use dialoguer::Confirm;

    // If not in a TTY or in CI, default to NO
    if !crate::tools::is_tty() || std::env::var("CI").is_ok() {
        return Ok(false);
    }

    // Default is NO (false) for downgrades
    let proceed = Confirm::new()
        .with_prompt("  Do you want to replace it with an older version?")
        .default(false)
        .interact()
        .unwrap_or(false);

    Ok(proceed)
}

/// Copy zv binary and regenerate shims
/// This ensures that shims point to the correct binary
async fn copy_binary_and_regenerate_shims(
    source: &Path,
    target: &Path,
    app: &crate::App,
    quiet: bool,
) -> crate::Result<()> {
    use color_eyre::eyre::Context;

    // Ensure bin directory exists using app's canonical path
    tokio::fs::create_dir_all(app.bin_path())
        .await
        .with_context(|| format!("Failed to create directory {}", app.bin_path().display()))?;

    tokio::fs::copy(source, target).await.with_context(|| {
        format!(
            "Failed to copy zv binary from {} to {}",
            source.display(),
            target.display()
        )
    })?;

    // Regenerate shims to ensure they point to the correct zv binary
    let toolchain_manager = &app.toolchain_manager;
    if let Some(install) = toolchain_manager.get_active_install() {
        toolchain_manager
            .deploy_shims(install, true, quiet)
            .await
            .with_context(|| "Failed to regenerate shims after updating zv binary")?;
    }

    Ok(())
}
