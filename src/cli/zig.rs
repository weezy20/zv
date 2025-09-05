use color_eyre::eyre::{bail, eyre};
use std::process::{Command, Stdio};
use std::path::PathBuf;
use crate::{ZigVersion, App, UserConfig, Shell, tools};

const MAX_RECURSION: u32 = 10;

pub fn zig_main() -> crate::Result<()> {
    // Recursion guard
    let recursion_count: u32 = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Collect command line arguments
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0); // drop program name

    // Check for +version override
    let version_override = args.iter()
        .position(|arg| arg.starts_with('+'))
        .map(|pos| args.remove(pos))
        .map(|arg| arg.strip_prefix('+').unwrap().to_string());

    let zig_path = if let Some(version_str) = version_override {
        // Parse the version override
        let version = version_str.parse::<ZigVersion>()
            .map_err(|e| eyre!("Invalid version override '+{}': {}", version_str, e))?;
        
        find_zig_for_version(&version)?
    } else {
        // Default to system zig or zv-managed zig
        find_default_zig()?
    };

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

    std::process::exit(status.code().unwrap_or(1));
}

/// Find the Zig executable for a specific version
fn find_zig_for_version(version: &ZigVersion) -> crate::Result<PathBuf> {
    // Initialize app to access zv directory structure
    let (zv_dir, _) = tools::fetch_zv_dir()?;
    let zv_dir = std::fs::canonicalize(&zv_dir).map_err(|e| eyre!("Failed to canonicalize zv dir: {}", e))?;
    
    let app = App::init(UserConfig {
        path: zv_dir.clone(),
        shell: Shell::detect(),
    }).map_err(|e| eyre!("Failed to initialize app: {}", e))?;

    match version {
        ZigVersion::Semver(v) => {
            // Look for installed version in zv directory
            let version_dir = zv_dir.join("versions").join(v.to_string());
            let zig_exe = if cfg!(windows) {
                version_dir.join("zig.exe")
            } else {
                version_dir.join("zig")
            };
            
            if zig_exe.exists() {
                Ok(zig_exe)
            } else {
                Err(eyre!("Zig version {} is not installed. Run 'zv install {}' first.", v, v))
            }
        }
        ZigVersion::Master(_) => {
            // Look for master build
            let master_dir = zv_dir.join("versions").join("master");
            let zig_exe = if cfg!(windows) {
                master_dir.join("zig.exe")
            } else {
                master_dir.join("zig")
            };
            
            if zig_exe.exists() {
                Ok(zig_exe)
            } else {
                Err(eyre!("Zig master version is not installed. Run 'zv install master' first."))
            }
        }
        ZigVersion::Stable(_) | ZigVersion::Latest(_) => {
            // For stable/latest, we need to resolve to actual version first
            // For now, fall back to system zig
            Err(eyre!("Stable/latest version resolution not yet implemented. Use specific version."))
        }
        ZigVersion::Unknown => {
            Err(eyre!("Cannot use unknown version"))
        }
    }
}

/// Find the default Zig executable (zv-managed or system)
fn find_default_zig() -> crate::Result<PathBuf> {
    // Try to get zv-managed zig first
    if let Ok((zv_dir, _)) = tools::fetch_zv_dir() {
        if let Ok(zv_dir) = std::fs::canonicalize(&zv_dir) {
            if let Ok(app) = App::init(UserConfig {
                path: zv_dir,
                shell: Shell::detect(),
            }) {
                if let Some(zig_path) = app.zv_zig_or_system() {
                    return Ok(zig_path);
                }
            }
        }
    }
    
    // Fall back to system zig
    which::which("zig").map_err(|_| eyre!("Could not find zig executable"))
}
