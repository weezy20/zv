use crate::{App, UserConfig, ZigVersion, tools};
use color_eyre::eyre::{bail, eyre};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn zig_main() -> crate::Result<()> {
    // Recursion guard - check early to prevent infinite loops
    crate::check_recursion_with_context("zig proxy")?;

    // Collect command line arguments
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0); // drop program name

    // Check for +version override (only if it's the first argument)
    let version_override = if args.first().map_or(false, |arg| arg.starts_with('+')) {
        Some(args.remove(0).strip_prefix('+').unwrap().to_string())
    } else {
        None
    };

    let zig_path = if let Some(version_str) = version_override {
        // Parse the version override
        let version = version_str
            .parse::<ZigVersion>()
            .map_err(|e| eyre!("Invalid version override '+{}': {}", version_str, e))?;

        find_zig_for_version(&version)?
    } else {
        // Default to system zig or zv-managed zig
        find_default_zig()?
    };

    // Get current recursion count for incrementing
    let recursion_count: u32 = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut child = Command::new(zig_path)
        .args(args)
        .env("ZV_RECURSION_COUNT", (recursion_count + 1).to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| eyre!("Failed to launch zig: {}", e))?;

    let status = child
        .wait()
        .map_err(|e| eyre!("Failed to wait for zig: {}", e))?;

    std::process::exit(status.code().unwrap_or(3));
}

/// Find the Zig executable for a specific version
fn find_zig_for_version(version: &ZigVersion) -> crate::Result<PathBuf> {
    // Get zv directory structure
    let (zv_base_path, _) = tools::fetch_zv_dir()?;

    match version {
        ZigVersion::Semver(v) => {
            // Look for installed version in zv directory
            let version_dir = zv_base_path.join("versions").join(v.to_string());
            let zig_exe = if cfg!(windows) {
                version_dir.join("zig.exe")
            } else {
                version_dir.join("zig")
            };

            if zig_exe.exists() {
                Ok(zig_exe)
            } else {
                Err(eyre!(
                    "Zig version {} is not installed. Run 'zv install {}' first.",
                    v,
                    v
                ))
            }
        }
        ZigVersion::Master(_) => {
            // Look for master build
            let master_dir = zv_base_path.join("versions").join("master");
            let zig_exe = if cfg!(windows) {
                master_dir.join("zig.exe")
            } else {
                master_dir.join("zig")
            };

            if zig_exe.exists() {
                Ok(zig_exe)
            } else {
                Err(eyre!(
                    "Zig master version is not installed. Run 'zv install master' first."
                ))
            }
        }
        ZigVersion::Stable(_) | ZigVersion::Latest(_) => {
            // For stable/latest, we need to resolve to actual version first
            // For now, fall back to system zig
            Err(eyre!(
                "Stable/latest version resolution not yet implemented. Use specific version."
            ))
        }
    }
}

/// Find the default Zig executable (zv-managed or system)
fn find_default_zig() -> crate::Result<PathBuf> {
    // Try to get zv-managed zig first
    if let Ok((zv_base_path, _)) = tools::fetch_zv_dir() {
        if let Ok(app) = App::init(UserConfig {
            zv_base_path,
            shell: None,
        }) {
            if let Some(zig_path) = app.zv_zig() {
                return Ok(zig_path);
            }
        }
    }

    // Fall back to system zig
    bail!("Could not find zig executable")
}
