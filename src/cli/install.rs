use std::collections::HashMap;

use crate::app::network::ZigRelease;
use crate::{ResolvedZigVersion, ZigVersion, ZvError};
use crate::{app::App, cli::r#use::resolve_zig_version};
use color_eyre::eyre::{Context, Result, eyre};
use yansi::Paint;

/// Main entry point for the install command
pub(crate) async fn install_versions(
    zig_versions: Vec<ZigVersion>,
    app: &mut App,
    force_ziglang: bool,
) -> Result<()> {
    if zig_versions.is_empty() {
        return Err(eyre!(
            "At least one version must be specified. e.g., 'zv install latest' or 'zv install 0.15.1,master'"
        ));
    }

    let is_single_version = zig_versions.len() == 1;
    let should_set_active = is_single_version && app.toolchain_manager.installations_empty();

    if should_set_active {
        println!(
            "📦 Installing {} (will be set as active zig)...",
            Paint::blue(&zig_versions[0].to_string())
        );
    } else if is_single_version {
        println!(
            "📦 Installing {}...",
            Paint::blue(&zig_versions[0].to_string())
        );
    } else {
        println!(
            "📦 Installing {} versions...",
            Paint::blue(&zig_versions.len().to_string())
        );
    }

    // Deduplicate semver variants before resolution
    // e.g., latest@0.14.0, stable@0.14.0, 0.14.0 all become just 0.14.0
    let zig_versions = crate::tools::deduplicate_semver_variants(zig_versions);

    // First, resolve all versions to detect duplicates and store their ZigRelease objects
    let mut resolved_map: HashMap<ResolvedZigVersion, ZigRelease> = HashMap::new();
    let mut resolution_errors = Vec::new();

    for zig_version in zig_versions {
        match resolve_zig_version(app, &zig_version).await {
            Ok(resolved) => {
                // Get the ZigRelease that was set by resolve_zig_version
                let zig_release = app.to_install.take().ok_or_else(|| {
                    eyre!("Internal error: resolve_zig_version did not set to_install")
                })?;

                resolved_map.entry(resolved).or_insert(zig_release);
            }
            Err(e) => {
                let error_msg = match e {
                    ZvError::ZigVersionResolveError(err) => ZvError::ZigVersionResolveError(eyre!(
                        "Failed to resolve version '{}': {}. Try running 'zv sync' to update the index or 'zv list' to see available versions.",
                        zig_version,
                        err
                    )),
                    _ => e,
                };
                eprintln!(
                    "❌ Failed to resolve {}: {}",
                    Paint::red(&zig_version.to_string()),
                    error_msg
                );
                resolution_errors.push((zig_version, error_msg));
            }
        }
    }

    // If all resolutions failed, return early
    if resolved_map.is_empty() {
        return Err(eyre!("Failed to resolve any versions"));
    }

    let mut installed_versions = Vec::new();
    let mut failed_versions = Vec::new();

    // Process each unique resolved version
    for (resolved_version, zig_release) in resolved_map {
        match install_resolved_version(
            &resolved_version,
            zig_release,
            app,
            force_ziglang,
            should_set_active,
        )
        .await
        {
            Ok(()) => {
                installed_versions.push(resolved_version);
            }
            Err(e) => {
                eprintln!(
                    "❌ Failed to install {}: {}",
                    Paint::red(&resolved_version.to_string()),
                    e
                );
                failed_versions.push((resolved_version, e));
            }
        }
    }

    // Report results
    if !installed_versions.is_empty() {
        println!();
        for resolved in &installed_versions {
            if should_set_active {
                println!(
                    "✅ Installed and activated: {}",
                    Paint::green(&resolved.version().to_string())
                );
            } else {
                println!(
                    "✅ Installed: {}",
                    Paint::green(&resolved.version().to_string())
                );
            }
        }
    }

    if !failed_versions.is_empty() {
        println!();
        eprintln!("❌ Failed installations:");
        for (version, _) in &failed_versions {
            eprintln!("  • {}", Paint::red(&version.to_string()));
        }
    }

    // If all installations failed, return an error
    if installed_versions.is_empty() {
        return Err(eyre!("All version installations failed"));
    }

    Ok(())
}

/// Install a single Zig version that has already been resolved
async fn install_resolved_version(
    resolved_version: &ResolvedZigVersion,
    zig_release: ZigRelease,
    app: &mut App,
    force_ziglang: bool,
    set_active: bool,
) -> Result<()> {
    // Check if already installed
    if let Some(p) = app.check_installed(resolved_version) {
        if set_active {
            app.set_active_version(resolved_version, Some(p)).await?;
        }
        // Version already installed, just return success
        return Ok(());
    }

    // Set the ZigRelease for installation
    app.to_install = Some(zig_release);

    // Now install with the correctly set app.to_install
    app.install_release(force_ziglang).await.wrap_err_with(|| {
        format!(
            "Failed to download and install Zig version {}",
            resolved_version
        )
    })?;

    // Set as active if this is the special case (single version, no prior installations)
    if set_active {
        app.set_active_version(resolved_version, None).await?;
    }

    Ok(())
}
