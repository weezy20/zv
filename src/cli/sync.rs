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

    println!("{}", "Syncing zv...".cyan());

    // Ensure data/config/cache directories exist
    ensure_directories(app).await?;

    // Check and update zv binary (self-install to internal bin)
    println!("  {} Checking zv binary...", "→".blue());
    let binary_updated = check_and_update_zv_binary(app, false).await?;

    // Create public bin symlinks (belt-and-suspenders)
    #[cfg(unix)]
    if let Some(pub_bin) = app.public_bin_path() {
        create_public_bin_symlinks(app.bin_path(), pub_bin).await?;
    }

    // Run migrations if binary was actually updated
    if binary_updated
        && let Err(e) = crate::app::migrations::migrate(app.path(), &app.paths.config_file).await
    {
        eprintln!("  {} Warning: Migration failed: {}", "⚠".yellow(), e);
    }

    // Fetch zig index
    println!("  {} Refreshing Zig index...", "→".blue());
    app.sync_zig_index().await?;
    println!("  {} Zig index synced successfully", "✓".green());

    // Fetch mirrors list
    println!("  {} Refreshing community mirrors...", "→".blue());
    let mirror_count = app.sync_mirrors().await?;
    println!(
        "  {} Community mirrors synced successfully ({} mirrors)",
        "✓".green(),
        mirror_count
    );

    // Backfill ZLS mappings for any locally installed Zig versions we haven't seen yet.
    // Network-only: no binaries are downloaded or built here. Failures per-version are
    // logged and skipped so one API hiccup can't fail the whole sync.
    backfill_zls_mappings(app).await;

    // Re-assert shims (zig + zls) for the active install. Idempotent — covers the case
    // where the zv binary was already up to date so `copy_binary_and_regenerate_shims`
    // did not run deploy_shims itself.
    if let Some(install) = app.toolchain_manager.get_active_install() {
        app.toolchain_manager
            .deploy_shims(install, true, true)
            .await?;
    }

    println!("{}", "Sync completed successfully!".green().bold());

    // On Tier 2/3 (macOS Library or ZV_DIR), warn if PATH not configured
    if !app.source_set {
        #[cfg(target_os = "linux")]
        {
            // Linux Tier 1: should never happen since ~/.local/bin is in PATH
            let target = app
                .public_bin_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| app.bin_path().display().to_string());
            println!(
                "{} {} is not in your PATH. This is unusual on Linux.",
                "⚠".yellow(),
                Paint::cyan(&target)
            );
        }
        #[cfg(target_os = "macos")]
        {
            if app.paths.tier == 2 {
                println!(
                    "{} PATH not configured. Run {} to add zv to your PATH.",
                    "⚠".yellow(),
                    Paint::blue("zv setup")
                );
            }
        }
        #[cfg(windows)]
        {
            println!(
                "{} PATH not configured. Run {} to add zv to your PATH.",
                "⚠".yellow(),
                Paint::blue("zv setup")
            );
        }
    }

    Ok(())
}

async fn ensure_directories(app: &crate::App) -> crate::Result<()> {
    use std::path::Path;

    async fn ensure(dir: &Path) -> crate::Result<()> {
        if !dir.try_exists().unwrap_or(false)
            && let Some(parent) = dir.parent()
            && parent.exists()
        {
            tokio::fs::create_dir_all(dir).await?;
        }
        Ok(())
    }

    ensure(&app.paths.data_dir).await?;
    ensure(&app.paths.config_dir).await?;
    ensure(&app.paths.cache_dir).await?;
    ensure(app.bin_path()).await?;

    if let Some(ref pub_dir) = app.paths.public_bin_dir {
        ensure(pub_dir).await?;
    }

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

    // Ensure internal bin directory exists
    tokio::fs::create_dir_all(app.bin_path())
        .await
        .with_context(|| format!("Failed to create directory {}", app.bin_path().display()))?;

    // Remove the target first to avoid ETXTBSY on Linux when the binary is running
    if target.exists() {
        tokio::fs::remove_file(target)
            .await
            .with_context(|| format!("Failed to remove existing binary at {}", target.display()))?;
    }

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

    // On XDG systems, keep public symlinks in ~/.local/bin up to date
    #[cfg(unix)]
    if let Some(pub_bin) = app.public_bin_path() {
        create_public_bin_symlinks(app.bin_path(), pub_bin)
            .await
            .with_context(|| {
                format!(
                    "Failed to create public bin symlinks in {}",
                    pub_bin.display()
                )
            })?;
    }

    Ok(())
}

/// Create (or refresh) symlinks in the public bin dir (`~/.local/bin`) pointing at
/// the internal bin dir (`ZV_DIR/bin`).  Only called on XDG-capable systems.
///
/// Layout produced:
/// ```text
/// ~/.local/bin/zv  → ZV_DIR/bin/zv
/// ~/.local/bin/zig → ZV_DIR/bin/zig   (only if shim exists)
/// ```
#[cfg(unix)]
async fn create_public_bin_symlinks(internal_bin: &Path, public_bin: &Path) -> crate::Result<()> {
    use crate::Shim;
    use color_eyre::eyre::Context;

    tokio::fs::create_dir_all(public_bin)
        .await
        .with_context(|| format!("Failed to create public bin dir {}", public_bin.display()))?;

    // Helper: create / replace a symlink link → target
    async fn place_symlink(target: &Path, link: &Path) -> crate::Result<()> {
        if link.exists() || link.is_symlink() {
            tokio::fs::remove_file(link).await?;
        }
        tokio::fs::symlink(target, link).await?;
        Ok(())
    }

    let zv_name = Shim::Zv.executable_name();
    let zv_src = internal_bin.join(zv_name);
    let zv_dst = public_bin.join(zv_name);
    if zv_src.exists() {
        place_symlink(&zv_src, &zv_dst)
            .await
            .with_context(|| format!("Failed to symlink zv in {}", public_bin.display()))?;
        tracing::debug!("Linked {} → {}", zv_dst.display(), zv_src.display());
    }

    let zig_name = Shim::Zig.executable_name();
    let zig_src = internal_bin.join(zig_name);
    let zig_dst = public_bin.join(zig_name);
    if zig_src.exists() {
        place_symlink(&zig_src, &zig_dst)
            .await
            .with_context(|| format!("Failed to symlink zig in {}", public_bin.display()))?;
        tracing::debug!("Linked {} → {}", zig_dst.display(), zig_src.display());
    }

    let zls_name = Shim::Zls.executable_name();
    let zls_src = internal_bin.join(zls_name);
    let zls_dst = public_bin.join(zls_name);
    if zls_src.exists() {
        place_symlink(&zls_src, &zls_dst)
            .await
            .with_context(|| format!("Failed to symlink zls in {}", public_bin.display()))?;
        tracing::debug!("Linked {} → {}", zls_dst.display(), zls_src.display());
    }

    Ok(())
}

/// Query the ZLS release-worker for every locally installed Zig version that does
/// not yet have a cached Zig→ZLS mapping, and persist the results to `zv.toml`.
///
/// This is a cache primer: no ZLS binaries are downloaded or built. When the user
/// later runs `zv zls`, the provisioning path can use the cached mapping without
/// another API round-trip.
async fn backfill_zls_mappings(app: &crate::App) {
    use crate::app::migrations::{ZlsConfig, ZvConfig, load_zv_config, save_zv_config};
    use futures::stream::{self, StreamExt};
    use std::collections::{HashMap, HashSet};
    use yansi::Paint;

    let installations = app.toolchain_manager.list_installations();
    if installations.is_empty() {
        return;
    }

    let existing_config = load_zv_config(&app.paths.config_file).ok();
    let existing_keys: HashSet<String> = existing_config
        .as_ref()
        .and_then(|c| c.zls.as_ref())
        .map(|z| z.mappings.keys().cloned().collect())
        .unwrap_or_default();

    let missing: Vec<String> = installations
        .iter()
        .map(|(v, _, _)| v.to_string())
        .filter(|k| !existing_keys.contains(k))
        .collect();

    if missing.is_empty() {
        println!("  {} ZLS mappings up to date", "✓".green());
        return;
    }

    println!(
        "  {} Resolving ZLS for {} Zig version(s)...",
        "→".blue(),
        missing.len()
    );

    const CONCURRENCY: usize = 4;
    let results: Vec<(String, Result<String, crate::ZvError>)> = stream::iter(missing)
        .map(|zig_ver| async move {
            let result = crate::app::network::zls::select_version(&zig_ver)
                .await
                .map(|r| r.version);
            (zig_ver, result)
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;

    let mut config = existing_config.unwrap_or(ZvConfig {
        version: env!("CARGO_PKG_VERSION").to_string(),
        active_zig: None,
        local_master_zig: None,
        zls: None,
    });
    config.version = env!("CARGO_PKG_VERSION").to_string();
    let zls_config = config.zls.get_or_insert(ZlsConfig {
        mappings: HashMap::new(),
    });

    let mut added = 0usize;
    let mut failed = 0usize;
    for (zig_ver, result) in results {
        match result {
            Ok(zls_ver) => {
                zls_config.mappings.insert(zig_ver, zls_ver);
                added += 1;
            }
            Err(e) => {
                tracing::debug!(
                    target: "zv::cli::sync",
                    "Failed to resolve compatible ZLS for Zig {}: {}",
                    zig_ver,
                    e
                );
                failed += 1;
            }
        }
    }

    if added > 0
        && let Err(e) = save_zv_config(&app.paths.config_file, &config)
    {
        println!(
            "  {} Failed to persist ZLS mappings: {}",
            "⚠".yellow(),
            e
        );
        return;
    }

    match (added, failed) {
        (a, 0) => println!(
            "  {} ZLS mappings: {} cached",
            "✓".green(),
            Paint::green(&a.to_string())
        ),
        (0, f) => println!(
            "  {} ZLS mapping lookup failed for {} version(s) (see ZV_LOG)",
            "⚠".yellow(),
            Paint::yellow(&f.to_string())
        ),
        (a, f) => println!(
            "  {} ZLS mappings: {} cached, {} failed",
            "✓".green(),
            Paint::green(&a.to_string()),
            Paint::yellow(&f.to_string())
        ),
    }
}
