//! Self-update command for zv binary using async GitHub API and self-replace crate
//!
//! Checks GitHub releases for newer versions and updates the binary if available.
//! Intelligently handles updates whether zv is running from ZV_DIR/bin or elsewhere.

use color_eyre::eyre::{Context, Result, bail, eyre};
use semver::Version;
use yansi::Paint;
use serde::Deserialize;
use std::{path::Path, time::Duration};
use tokio::task;

use crate::{App, tools, app::utils};
use walkdir::WalkDir;

/// Get the target triple used in GitHub release assets for the current platform
/// This is a fallback when CARGO_CFG_TARGET_TRIPLE is not available (which should be rare).
/// The targets here match the CI generated artifacts.
fn get_release_target() -> Option<&'static str> {
    use target_lexicon::{HOST, Architecture, OperatingSystem};

    match (HOST.architecture, HOST.operating_system) {
        // macOS
        (Architecture::X86_64, OperatingSystem::Darwin(_)) => {
            Some("x86_64-apple-darwin")
        }
        (Architecture::Aarch64(_), OperatingSystem::Darwin(_)) => {
            Some("aarch64-apple-darwin")
        }
        // Windows
        (Architecture::X86_64, OperatingSystem::Windows) => {
            Some("x86_64-pc-windows-msvc")
        }
        // Linux
        (Architecture::X86_64, OperatingSystem::Linux) => {
            // Default to GNU libc, could also be musl but GNU is more common
            Some("x86_64-unknown-linux-gnu")
        }
        (Architecture::Aarch64(_), OperatingSystem::Linux) => {
            Some("aarch64-unknown-linux-gnu")
        }
        // Unsupported combinations
        _ => None,
    }
}

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize, Debug)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

pub async fn update_zv(app: &mut App, force: bool) -> Result<()> {
    println!("{}", "Checking for zv updates...".cyan());

    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))
        .expect("CARGO_PKG_VERSION should be valid semver");

    println!("Current version: {}", Paint::yellow(&current_version));

    // Get target triple for this platform
    // Prefer the compilation target (embedded at build time) over runtime detection
    let target = option_env!("CARGO_CFG_TARGET_TRIPLE")
        .or_else(|| get_release_target())
        .ok_or_else(|| eyre!("Unable to determine target platform for updates"))?;

    println!("  {} Detected platform: {}", "→".blue(), target);

    // Fetch latest release from GitHub API
    let client = reqwest::Client::builder()
        .user_agent(utils::zv_agent())
        .connect_timeout(Duration::from_secs(*crate::app::FETCH_TIMEOUT_SECS))
        .build()
        .wrap_err("Failed to create HTTP client")?;

    let latest_release = fetch_latest_release(&client).await
        .wrap_err("Failed to fetch latest release from GitHub")?;

    // Parse version from tag_name (remove 'v' prefix if present)
    let version_str = latest_release.tag_name.strip_prefix('v').unwrap_or(&latest_release.tag_name);
    let latest_version = Version::parse(version_str)
        .wrap_err("Failed to parse latest version from GitHub release tag")?;

    println!(
        "  {} Latest version from releases:  {}",
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

    // Find the correct asset for this platform
    // The naming convention is: zv-{target}.{extension}
    let asset = if cfg!(windows) {
        // Windows: only look for .zip
        let expected_asset_name = format!("zv-{target}.zip");
        latest_release
            .assets
            .iter()
            .find(|asset| asset.name == expected_asset_name)
            .ok_or_else(|| {
                let available_assets: Vec<&str> = latest_release
                    .assets
                    .iter()
                    .map(|a| a.name.as_str())
                    .filter(|name| name.starts_with("zv-") && name.ends_with(".zip"))
                    .collect();
                
                eyre!(
                    "No compatible release asset found for platform: {} (expected: {})\nAvailable assets: {:?}",
                    target,
                    expected_asset_name,
                    available_assets
                )
            })?
    } else {
        // Unix: prefer .tar.gz, fallback to .tar.xz
        let gz_asset_name = format!("zv-{target}.tar.gz");
        let xz_asset_name = format!("zv-{target}.tar.xz");
        
        latest_release
            .assets
            .iter()
            .find(|asset| asset.name == gz_asset_name)
            .or_else(|| {
                latest_release
                    .assets
                    .iter()
                    .find(|asset| asset.name == xz_asset_name)
            })
            .ok_or_else(|| {
                let available_assets: Vec<&str> = latest_release
                    .assets
                    .iter()
                    .map(|a| a.name.as_str())
                    .filter(|name| {
                        name.starts_with("zv-") && (name.ends_with(".tar.gz") || name.ends_with(".tar.xz"))
                    })
                    .collect();
                
                eyre!(
                    "No compatible release asset found for platform: {} (tried: {} and {})\nAvailable assets: {:?}",
                    target,
                    gz_asset_name,
                    xz_asset_name,
                    available_assets
                )
            })?
    };

    tracing::trace!(target: "zv::update", "  {} Found asset: {}", "→".blue(), asset.name);

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
        // Download and replace the binary in place
        println!("  {} Downloading and installing update...", "→".blue());

        let _temp_dir = download_and_replace_binary(&client, asset, &expected_zv_path, true).await
            .wrap_err("Failed to update zv")?;
        // Keep _temp_dir alive until after self_replace completes

        println!(
            "  {} Updated successfully to zv {}!",
            "✓".green(),
            latest_version
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
        // Download to temp location and then copy to ZV_DIR
        println!(
            "  {} Running from outside ZV_DIR, downloading to temporary location...",
            "→".blue()
        );

        // Use tempfile to create a temporary directory that will be cleaned up automatically
        let temp_dir = tempfile::Builder::new()
            .prefix("zv-update-")
            .tempdir()
            .wrap_err("Failed to create temporary directory")?;

        let temp_binary = temp_dir
            .path()
            .join(if cfg!(windows) { "zv.exe" } else { "zv" });

        // Download the binary to temp location
        let _extract_temp_dir = download_and_replace_binary(&client, asset, &temp_binary, false).await
            .wrap_err("Failed to download binary to temporary location")?;
        // Keep _extract_temp_dir alive until after copy completes

        println!("  {} Downloaded version {}", "✓".green(), latest_version);
        
        // Copy the new binary to ZV_DIR/bin/zv
        println!("  {} Installing ...", "→".blue());
        
        // Ensure ZV_DIR/bin exists
        if let Some(parent) = expected_zv_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .wrap_err_with(|| format!("Failed to create ZV_DIR/bin directory: {}", parent.display()))?;
        }
        
        // Copy the binary
        tokio::fs::copy(&temp_binary, &expected_zv_path).await
            .wrap_err("Failed to copy binary to ZV_DIR")?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = tokio::fs::set_permissions(&expected_zv_path, std::fs::Permissions::from_mode(0o755)).await {
                tools::warn(format!("Failed to set binary permissions: {}", e));
            }
        }

        println!(
            "  {} Updated successfully to zv {}!",
            "✓".green(),
            latest_version
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
    }

    println!("\n{} {}", "✓".green(), "Update complete".green().bold());

    Ok(())
}

/// Fetch the latest release from GitHub API
async fn fetch_latest_release(client: &reqwest::Client) -> Result<GitHubRelease> {
    let url = "https://api.github.com/repos/weezy20/zv/releases/latest";
    
    let response = client
        .get(url)
        .send()
        .await
        .wrap_err("Failed to send request to GitHub API")?;

    if !response.status().is_success() {
        bail!("GitHub API request failed with status: {}", response.status());
    }

    let release = response
        .json::<GitHubRelease>()
        .await
        .wrap_err("Failed to parse GitHub API response")?;

    Ok(release)
}

/// Download and extract/replace the binary from a GitHub release asset
/// Returns the temporary directory handle to keep it alive until the caller is done
async fn download_and_replace_binary(
    client: &reqwest::Client,
    asset: &GitHubAsset,
    target_path: &Path,
    use_self_replace: bool,
) -> Result<tempfile::TempDir> {
    // Create a temporary file for the download with the correct extension
    let temp_dir = tempfile::tempdir()
        .wrap_err("Failed to create temporary directory for download")?;
    let temp_file_path = temp_dir.path().join(&asset.name);

    // Download the asset
    println!("  {} Downloading {}...", "→".blue(), asset.name);
    
    let response = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .wrap_err("Failed to download release asset")?;

    if !response.status().is_success() {
        bail!("Failed to download asset: HTTP {}", response.status());
    }

    // Write to temporary file
    let mut file = tokio::fs::File::create(&temp_file_path).await
        .wrap_err("Failed to create temporary download file")?;

    let mut stream = response.bytes_stream();
    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.wrap_err("Failed to read download chunk")?;
        file.write_all(&chunk).await
            .wrap_err("Failed to write download chunk")?;
    }

    file.sync_all().await.wrap_err("Failed to sync download file to disk")?;
    drop(file); // Close the file

    println!("  {} Extracting binary...", "→".blue());

    // Extract the binary from the archive
    let temp_extract_dir = tempfile::tempdir()
        .wrap_err("Failed to create temporary extraction directory")?;

    extract(&temp_file_path, temp_extract_dir.path()).await?;

    // Find the zv binary in the extracted files
    // Based on cargo-dist:
    // - Unix archives (.tar.gz/.tar.xz) have a subdirectory like "zv-{target}/zv"
    // - Windows archives (.zip) extract files directly to the temp directory
    let target = option_env!("CARGO_CFG_TARGET_TRIPLE")
        .unwrap_or_else(|| get_release_target().unwrap_or("unknown"));
    let binary_name = if cfg!(windows) { "zv.exe" } else { "zv" };
    
    // Try the subdirectory structure first (Unix archives)
    let mut extracted_binary = temp_extract_dir
        .path()
        .join(format!("zv-{target}"))
        .join(binary_name);

    // If not found, try the root of the extraction directory (Windows zip)
    if !extracted_binary.is_file() {
        extracted_binary = temp_extract_dir.path().join(binary_name);
    }

    if !extracted_binary.is_file() {
        println!("  {} Debug: Listing extracted contents...", "→".blue());
        // walkdir does the recursion for you
        for entry in WalkDir::new(temp_extract_dir.path())
            .into_iter()
            .filter_map(Result::ok)
        {
            let depth = entry.depth();
            let prefix = "  ".repeat(1 + depth); // 1 base indent + 2 spaces per level
            println!("{prefix}{}", entry.path().display());
        }

        bail!("Could not find zv binary in extracted archive at: {}", extracted_binary.display());
    }

    // Install the binary
    println!("  {} Installing update...", "→".blue());
    
    // Ensure target directory exists
    if let Some(parent) = target_path.parent() {
        tokio::fs::create_dir_all(parent).await
            .wrap_err("Failed to create target directory")?;
    }

    if use_self_replace {
        // Use self-replace to atomically replace the binary (when running from ZV_DIR)
        self_replace::self_replace(&extracted_binary)
            .wrap_err("Failed to replace binary with updated version")?;
    } else {
        tokio::fs::copy(&extracted_binary, target_path).await
            .wrap_err("Failed to copy binary to target location")?;
        
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(e) = tokio::fs::set_permissions(target_path, std::fs::Permissions::from_mode(0o755)).await {
                tools::warn(format!("Failed to set binary permissions: {}", e));
            }
        }
    }

    Ok(temp_extract_dir)
}

/// Dispatch extraction for zip/tar.xz/tar.gz
async fn extract(archive: &Path, dest: &Path) -> Result<()> {
    let ext = archive.extension().and_then(|e| e.to_str()).unwrap_or_default();
    let ext2 = archive.file_stem()
        .and_then(|n| n.to_str()?.rsplit_once('.'))
        .map(|(_, e)| e); // foo.tar.gz  ->  tar

    match (ext, ext2) {
        ("gz", Some("tar")) => extract_tar(archive, dest, TarDecoder::Gz).await,
        ("xz", Some("tar")) => extract_tar(archive, dest, TarDecoder::Xz).await,
        ("zip", _) => extract_zip(archive, dest).await,
        _ => bail!("Unsupported archive type: {}", archive.display()),
    }
}

enum TarDecoder {
    Gz,
    Xz,
}

async fn extract_tar(archive: &Path, dest: &Path, decoder: TarDecoder) -> Result<()> {
    let archive = archive.to_owned();
    let dest = dest.to_owned();

    task::spawn_blocking(move || {
        let file = std::fs::File::open(&archive).wrap_err("Failed to open tar archive")?;

        let boxed_decoder: Box<dyn std::io::Read> = match decoder {
            TarDecoder::Gz => Box::new(flate2::read::GzDecoder::new(file)),
            TarDecoder::Xz => Box::new(xz2::read::XzDecoder::new(file)),
        };

        let mut archive = tar::Archive::new(boxed_decoder);
        archive.unpack(&dest).wrap_err("Failed to unpack tar archive")?;

        Ok(())
    })
    .await
    .wrap_err("tar extraction task panicked")?
}

async fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let archive = archive.to_owned();
    let dest = dest.to_owned();

    task::spawn_blocking(move || {
        let file = std::fs::File::open(&archive).wrap_err("Failed to open zip archive")?;
        let mut zip = zip::ZipArchive::new(file).wrap_err("Failed to read zip archive")?;

        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let out_path = match entry.enclosed_name() {
                Some(p) => dest.join(p),
                None => continue,
            };

            if entry.name().ends_with('/') {
                std::fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out)?;
            }

            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))?;
            }
        }

        Ok(())
    })
    .await
    .wrap_err("zip extraction task panicked")?
}
