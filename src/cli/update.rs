//! Self-update command for zv binary using self_update crate
//!
//! Checks GitHub releases for newer versions and updates the binary if available.
//! Intelligently handles updates whether zv is running from ZV_DIR/bin or elsewhere.

use color_eyre::eyre::{Context, Result, bail};
use semver::Version;
use yansi::Paint;

use crate::{App, tools};

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
        println!("  • You can try: {}", "cargo install zv".cyan().underline());
        println!(
            "  • Then run: {} to update bin @ ZV_DIR/bin/zv",
            "$CARGO_HOME/bin/zv sync".cyan().underline()
        );
        println!(
            "  • Then uninstall cargo binary: {}",
            "cargo uninstall zv".cyan().underline()
        );
        bail!("No release asset found for platform: {target} with extension {expected_extension}");
    }

    // Check if we're running from ZV_DIR/bin/zv or somewhere else
    let current_exe = std::env::current_exe().wrap_err("Failed to get current executable path")?;
    let (zv_dir, _) = tools::fetch_zv_dir()?;
    let expected_zv_path = zv_dir
        .join("bin")
        .join(if cfg!(windows) { "zv.exe" } else { "zv" });

    let running_from_zv_dir = tools::canonicalize(&current_exe)
        .ok()
        .and_then(|ce| {
            tools::canonicalize(&expected_zv_path)
                .ok()
                .map(|ez| ce == ez)
        })
        .unwrap_or(false);

    if running_from_zv_dir {
        // Standard case: running from ZV_DIR/bin/zv
        // Use self_update to replace the binary in place
        println!("  {} Downloading and installing update...", "→".blue());

        update_builder.bin_install_path(&expected_zv_path.parent().unwrap());
        let status = update_builder.build()?.update()?;

        println!(
            "  {} Updated successfully to version {}!",
            "✓".green(),
            status.version()
        );

        // Regenerate shims to ensure zig/zls symlinks point to the updated zv binary
        if let Some(install) = app.toolchain_manager.get_active_install() {
            println!("  {} Regenerating shims...", "→".blue());
            app.toolchain_manager
                .deploy_shims(install, true)
                .await
                .wrap_err("Failed to regenerate shims after update")?;
            println!("  {} Shims regenerated successfully", "✓".green());
        }
    } else {
        // Running from outside ZV_DIR (e.g., cargo install, custom location)
        // Download to temp location and exec into `zv sync`
        println!(
            "  {} Running from outside ZV_DIR, downloading to temporary location...",
            "→".blue()
        );

        let temp_dir = std::env::temp_dir().join(format!("zv-update-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir)?;

        // Download the binary to temp location
        update_builder.bin_install_path(&temp_dir);
        let status = update_builder.build()?.update()?;

        let temp_binary = temp_dir.join(if cfg!(windows) { "zv.exe" } else { "zv" });

        println!("  {} Downloaded version {}", "✓".green(), status.version());
        println!(
            "  {} Running sync to update ZV_DIR/bin/zv and regenerate shims...",
            "→".blue()
        );

        // Exec into the new binary with sync command
        // This will copy the new binary to ZV_DIR/bin/zv and regenerate shims
        exec_new_binary_with_sync(&temp_binary)?;

        // Never reached
        unreachable!()
    }

    println!();
    println!("{}", "Update completed successfully!".green().bold());

    Ok(())
}

/// Replace the current process with the newly downloaded binary running `sync`
fn exec_new_binary_with_sync(binary_path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        let err = std::process::Command::new(binary_path).arg("sync").exec();

        // exec only returns on error
        Err(err).wrap_err("Failed to exec into new binary for sync")
    }

    #[cfg(windows)]
    {
        use std::process::Stdio;

        let mut child = std::process::Command::new(binary_path)
            .arg("sync")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .wrap_err("Failed to spawn new binary for sync")?;

        let status = child.wait()?;

        if !status.success() {
            bail!("Sync command failed after update");
        }

        std::process::exit(0);
    }
}
