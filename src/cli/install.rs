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

    // Check if this is the special case: single version install with no prior installations
    let is_single_version = zig_versions.len() == 1;
    let should_set_active = is_single_version && app.toolchain_manager.installations_empty();

    if should_set_active {
        println!(
            "📦 Installing {} (will be set as active since no other versions are installed)...",
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

    let mut installed_versions = Vec::new();
    let mut failed_versions = Vec::new();

    // Process each version
    for zig_version in zig_versions {
        match install_single_version(&zig_version, app, force_ziglang, should_set_active).await {
            Ok(resolved_version) => {
                installed_versions.push((zig_version, resolved_version));
            }
            Err(e) => {
                eprintln!(
                    "❌ Failed to install {}: {}",
                    Paint::red(&zig_version.to_string()),
                    e
                );
                failed_versions.push((zig_version, e));
            }
        }
    }

    // Report results
    if !installed_versions.is_empty() {
        println!();
        for (_original, resolved) in &installed_versions {
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

/// Install a single Zig version
async fn install_single_version(
    zig_version: &ZigVersion,
    app: &mut App,
    force_ziglang: bool,
    set_active: bool,
) -> Result<ResolvedZigVersion> {
    // Resolve ZigVersion to a validated ResolvedZigVersion
    let resolved_version = resolve_zig_version(app, zig_version).await
        .map_err(|e| {
            match e {
                ZvError::ZigVersionResolveError(err) => {
                    ZvError::ZigVersionResolveError(eyre!(
                        "Failed to resolve version '{}': {}. Try running 'zv sync' to update the index or 'zv list' to see available versions.",
                        zig_version, err
                    ))
                }
                _ => e,
            }
        })?;

    // Check if already installed 
    if let Some(p) = app.check_installed(&resolved_version) {
        if set_active {
            // Set as active if this is the special case
            app.set_active_version(&resolved_version, Some(p)).await?;
        }
        // Version already installed, just return success
        return Ok(resolved_version);
    }

    // Install the version
    app.install_release(force_ziglang).await.wrap_err_with(|| {
        format!(
            "Failed to download and install Zig version {}",
            resolved_version
        )
    })?;

    // Set as active if this is the special case (single version, no prior installations)
    if set_active {
        app.set_active_version(&resolved_version, None).await?;
    }

    Ok(resolved_version)
}
