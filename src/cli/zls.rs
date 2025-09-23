use crate::{App, UserConfig, ZigVersion, tools};
use color_eyre::eyre::{bail, eyre};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn zls_main() -> crate::Result<()> {
    // Recursion guard - check early to prevent infinite loops
    crate::check_recursion_with_context("zls proxy")?;

    // Collect command line arguments
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0); // drop program name

    let zls_path = find_compatible_zls()?;

    // Get current recursion count for incrementing
    let recursion_count: u32 = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut child = Command::new(zls_path)
        .args(args)
        .env("ZV_RECURSION_COUNT", (recursion_count + 1).to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| eyre!("Failed to launch ZLS: {}", e))?;

    let status = child
        .wait()
        .map_err(|e| eyre!("Failed to wait for ZLS: {}", e))?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Find a compatible ZLS executable for the current Zig version
fn find_compatible_zls() -> crate::Result<PathBuf> {
    // Initialize app to access zv directory structure
    let (zv_base_path, _) = tools::fetch_zv_dir()?;

    let mut app = App::init(UserConfig {
        zv_base_path,
        shell: None,
    })
    .map_err(|e| eyre!("Failed to initialize app: {}", e))?;

    // Get the currently active Zig version
    let zig_version = get_current_zig_version(&app)?;

    // Try to find or fetch a compatible ZLS version
    match app.fetch_compatible_zls(&zig_version) {
        Ok(zls_path) => Ok(zls_path),
        Err(e) => {
            tracing::warn!("Failed to find compatible ZLS: {}", e);
            Err(eyre!(
                "No compatible ZLS found and no system ZLS available: {}",
                e
            ))
        }
    }
}

/// Get the current active Zig version
fn get_current_zig_version(app: &App) -> crate::Result<ZigVersion> {
    // Try to get version from currently active zv-managed zig
    if let Some(zig_path) = app.zv_zig() {
        match get_zig_version_from_executable(&zig_path) {
            Ok(version) => return Ok(version),
            Err(e) => {
                tracing::warn!(
                    "Failed to get version from zig executable {}: {}",
                    zig_path.display(),
                    e
                );
            }
        }
    }
    bail!("No Zig installation found");
}

/// Extract version information from a Zig executable
fn get_zig_version_from_executable(zig_path: &PathBuf) -> crate::Result<ZigVersion> {
    let output = Command::new(zig_path)
        .arg("version")
        .output()
        .map_err(|e| eyre!("Failed to execute zig version: {}", e))?;

    if !output.status.success() {
        return Err(eyre!("zig version command failed"));
    }

    let version_str = String::from_utf8(output.stdout)
        .map_err(|e| eyre!("Invalid UTF-8 in zig version output: {}", e))?
        .trim()
        .to_string();

    // Parse the version string
    version_str
        .parse::<ZigVersion>()
        .map_err(|e| eyre!("Failed to parse Zig version '{}': {}", version_str, e))
}
