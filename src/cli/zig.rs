use crate::cli::r#use::resolve_zig_version;
use crate::{App, UserConfig, ZigVersion, ZvError, tools};
use color_eyre::eyre::{Context, bail, eyre};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub async fn zig_main() -> crate::Result<()> {
    // Recursion guard - check early to prevent infinite loops
    crate::check_recursion_with_context("zig proxy")?;

    // Collect command line arguments
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0); // drop program name

    // Check for +version override (only if it's the first argument)
    let inline_version_override = if args.first().is_some_and(|arg| arg.starts_with('+')) {
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

        find_zig_for_version(&zv).await?
    } else {
        // Check for .zigversion file in current directory
        if let Some((zv, file)) = find_zigversion_from_file() {
            find_zig_for_version(&zv).await.wrap_err(eyre!(
                "Failed to find zig for version {zv} from file {}",
                file.display(),
            ))?
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
async fn find_zig_for_version(zig_version: &ZigVersion) -> crate::Result<PathBuf> {
    // Get zv directory structure
    let (zv_base_path, _) = tools::fetch_zv_dir()?;
    let mut app = App::init(UserConfig {
        zv_base_path,
        shell: None,
    })
    .await?;
    // Resolve ZigVersion to a validated ResolvedZigVersion
    // This already does all the validation and fetching we need
    let resolved_version = resolve_zig_version(&mut app, zig_version).await
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
    if let Some(p) = app.check_installed(&resolved_version) {
        Ok(p)
    } else {
        // Try installing with ziglang.org first, then fallback to mirrors
        let zig_exe = match app.install_release(true).await {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!("Failed to install zig version {}: {}", resolved_version, e);
                tracing::warn!("Retrying with community mirrors...");

                // We need to re-resolve the version since install_release consumed to_install
                let resolved_version_retry = resolve_zig_version(&mut app, zig_version).await
                    .map_err(|e| {
                        match e {
                            ZvError::ZigVersionResolveError(err) => {
                                ZvError::ZigVersionResolveError(eyre!(
                                    "Failed to resolve version '{}' for retry: {}. Try running 'zv sync' to update the index or 'zv list' to see available versions.",
                                    zig_version, err
                                ))
                            }
                            _ => e,
                        }
                    })?;

                app.install_release(false).await.map_err(|e| {
                    eyre!(
                        "Failed to download & install zig version {}: {}",
                        resolved_version_retry,
                        e
                    )
                })?
            }
        };

        Ok(zig_exe)
    }
}

/// Find the default Zig executable (zv-managed or system)
async fn find_default_zig() -> crate::Result<PathBuf> {
    // Try to get zv-managed zig first
    if let Ok((zv_base_path, _)) = tools::fetch_zv_dir()
        && let Ok(app) = App::init(UserConfig {
            zv_base_path,
            shell: None,
        })
        .await
        && let Some(zig_path) = app.zv_zig()
    {
        tracing::trace!(target: "zig", "Using zv-managed zig at {}", zig_path.display());
        return Ok(zig_path);
    }
    bail!("Could not find zig executable")
}

/// Search for a .zigversion file in the current directory or its ancestors
/// Returns the parsed ZigVersion if found beside a build.zig file
fn find_zigversion_from_file() -> Option<(ZigVersion, PathBuf)> {
    let mut current = std::env::current_dir().ok()?;

    loop {
        // Check if build.zig exists (project root marker)
        if current.join("build.zig").exists() {
            // Look for .zigversion in same directory
            let zigversion_file = current.join(".zigversion");
            if zigversion_file.exists() {
                return std::fs::read_to_string(&zigversion_file)
                    .ok()
                    .and_then(|s| {
                        s.trim()
                            .parse::<ZigVersion>()
                            .ok()
                            .map(|zv| (zv, zigversion_file))
                    });
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
