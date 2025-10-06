//! Self-update command for zv binary using self_update crate
//!
//! Checks GitHub releases for newer versions and updates the binary if available.
//! After successful update, regenerates shims for zig/zls.

use color_eyre::eyre::{Context, Result, bail};
use semver::Version;
use yansi::Paint;

use crate::App;

pub async fn update_zv(app: &mut App, force: bool) -> Result<()> {
    println!("{}", "Checking for zv updates...".cyan());

    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("CARGO_PKG_VERSION should be valid semver");

    println!("Current version: {}", Paint::yellow(&current_version));

    // Build the updater using self_update crate
    let mut update_builder = self_update::backends::github::Update::configure();

    update_builder
        .repo_owner("weezy20")
        .repo_name("zv")
        .bin_name("zv")
        .show_download_progress(true)
        .no_confirm(force)
        .current_version(env!("CARGO_PKG_VERSION"));

    let target = self_update::get_target();
    println!("  {} Detected platform: {}", "→".blue(), target);
    update_builder.target(&target);

    // Check what version is available
    let latest_release = match update_builder.build()?.get_latest_release() {
        Ok(release) => release,
        Err(e) => {
            bail!("Failed to fetch latest release: {}", e);
        }
    };

    let latest_version =
        Version::parse(&latest_release.version).wrap_err("Failed to parse latest version")?;

    println!(
        "  {} Latest version:  {}",
        "→".blue(),
        Paint::green(&latest_version)
    );

    // Compare versions
    if latest_version <= current_version && !force {
        println!("  {} Already up to date!", "✓".green());
        return Ok(());
    }

    if force && latest_version <= current_version {
        println!(
            "  {} Forcing reinstall of version {}",
            "→".blue(),
            latest_version
        );
    } else {
        println!(
            "  {} Update available: {} -> {}",
            "→".blue(),
            Paint::yellow(&current_version),
            Paint::green(&latest_version)
        );
    }

    // Check if a release asset exists for this platform
    // Windows uses .zip, Unix uses .tar.gz
    let expected_extension = if cfg!(windows) { ".zip" } else { ".tar.gz" };

    let has_asset = latest_release
        .assets
        .iter()
        .any(|asset| asset.name.contains(&target) && asset.name.ends_with(expected_extension));

    if !has_asset {
        println!(
            "  {} No compatible release asset found for this platform.",
            "✗".red()
        );
        println!("  • Build from source at https://github.com/weezy20/zv");
        println!("  • You can try: cargo install zv");
        println!("  • Then run: $CARGO_HOME/bin/zv sync to update bin @ ZV_DIR/bin/zv");
        println!("  • Then uninstall cargo binary: cargo uninstall zv");
        bail!("No release asset found for platform: {target} with extension {expected_extension}");
    }

    println!("  {} Downloading and installing update...", "→".blue());

    // Perform the update - this will:
    // 1. Download the correct asset for this platform
    // 2. Extract the binary
    // 3. Replace the current binary (ZV_DIR/bin/zv)
    let status = update_builder.build()?.update()?;

    println!(
        "  {} Updated successfully to version {}!",
        "✓".green(),
        status.version()
    );

    // Regenerate shims to ensure zig/zls symlinks point to the updated zv binary
    // Since self_update replaced ZV_DIR/bin/zv in place, we need to regenerate
    // the zig and zls shims that point to it
    if let Some(install) = app.toolchain_manager.get_active_install() {
        println!("  {} Regenerating shims...", "→".blue());
        app.toolchain_manager
            .deploy_shims(install, true)
            .await
            .wrap_err("Failed to regenerate shims after update")?;
        println!("  {} Shims regenerated successfully", "✓".green());
    }

    println!();
    println!("{}", "Update completed successfully!".green().bold());

    Ok(())
}
