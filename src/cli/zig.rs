use crate::{App, UserConfig, ZigVersion, tools};
use color_eyre::eyre::{bail, eyre};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub async fn zig_main() -> crate::Result<()> {
    // Recursion guard - check early to prevent infinite loops
    crate::check_recursion_with_context("zig proxy")?;

    // Collect command line arguments
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0); // drop program name

    // Check for +version override (only if it's the first argument)
    let inline_version_override = if args.first().map_or(false, |arg| arg.starts_with('+')) {
        Some(args.remove(0).strip_prefix('+').unwrap().to_string())
    } else {
        None
    };
    // Check for .zigversion file in current directory

    let zig_path = if let Some(version_str) = inline_version_override {
        // Parse the version override
        let zv = version_str
            .parse::<ZigVersion>()
            .map_err(|e| eyre!("Invalid version override '+{}': {}", version_str, e))?;

        find_zig_for_version(&zv)?
    } else {
        // Check for .zigversion file in current directory
        if let Some(zv) = find_zigversion_from_file() {
            find_zig_for_version(&zv)?
        }
        // Default to current active zig
        else {
            find_default_zig().await?
        }
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

    if let Some(code) = status.code() {
        std::process::exit(code);
    } else {
        // On Unix, process was terminated by signal
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                std::process::exit(128 + signal);
            }
        }
        std::process::exit(1);
    }
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
async fn find_default_zig() -> crate::Result<PathBuf> {
    // Try to get zv-managed zig first
    if let Ok((zv_base_path, _)) = tools::fetch_zv_dir() {
        if let Ok(app) = App::init(UserConfig {
            zv_base_path,
            shell: None,
        })
        .await
        {
            if let Some(zig_path) = app.zv_zig() {
                return Ok(zig_path);
            }
        }
    }

    // Fall back to system zig
    bail!("Could not find zig executable")
}

/// Search for a .zigversion file in the current directory or its ancestors
/// Returns the parsed ZigVersion if found beside a build.zig file
fn find_zigversion_from_file() -> Option<ZigVersion> {
    let mut current = std::env::current_dir().ok()?;

    loop {
        // Check if build.zig exists (project root marker)
        if current.join("build.zig").exists() {
            // Look for .zigversion in same directory
            let zigversion_file = current.join(".zigversion");
            if zigversion_file.exists() {
                return std::fs::read_to_string(zigversion_file)
                    .ok()
                    .and_then(|s| s.trim().parse::<ZigVersion>().ok());
            }
            break;
        }

        // Move up to parent directory
        if !current.pop() {
            break;
        }
    }

    None
}
