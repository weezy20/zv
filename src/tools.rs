use crate::{ZigVersion, ZvError};
use color_eyre::{
    Context as _, Result,
    eyre::{WrapErr, bail, eyre},
};
use std::{borrow::Cow, path::PathBuf};
use yansi::Paint;

/// Macro to print standardized solution suggestions with bullet points
/// 
/// Usage:
/// ```
/// suggest!("You can install a compatible Zig version with {}", cmd = "zv use <version>");
/// suggest!("Make sure you've run {}", cmd = "zv setup");
/// suggest!("Simple message without command");
/// ```
#[macro_export]
macro_rules! suggest {
    // Pattern with cmd parameter
    ($fmt:expr, cmd = $cmd:expr $(, $($args:tt)*)?) => {
        println!(
            "• {}",
            format!($fmt, $crate::tools::format_cmd($cmd) $(, $($args)*)?)
        );
    };
    // Pattern without cmd parameter
    ($fmt:expr $(, $($args:tt)*)?) => {
        println!("• {}", format!($fmt $(, $($args)*)?));
    };
}

/// Helper function to format commands with green italic styling
pub fn format_cmd(cmd: &str) -> String {
    Paint::green(cmd).italic().to_string()
}

/// Fetch the zv directory PATH set using env var or fallback PATH ($HOME/.zv)
/// This function also handles the initialization and creation of the ZV_DIR if it doesn't exist
/// Returns a canonicalized PathBuf and a bool indicating if the path was set via env var
pub(crate) fn fetch_zv_dir() -> Result<(PathBuf, bool)> {
    let zv_dir_env = match std::env::var("ZV_DIR") {
        Ok(dir) if !dir.is_empty() => Some(dir),
        Ok(_) => None,
        Err(env_err) => match env_err {
            std::env::VarError::NotPresent => None,
            std::env::VarError::NotUnicode(ref str) => {
                error(format!(
                    "Warning: ZV_DIR={str:?} is set but contains invalid Unicode."
                ));
                return Err(eyre!(env_err));
            }
        },
    };

    let (zv_dir, using_env) = if let Some(zv_dir) = zv_dir_env {
        (PathBuf::from(zv_dir), true /* using-env true */)
    } else {
        (
            ({
                dirs::home_dir()
                    .map(|home| home.join(".zv"))
                    .ok_or_else(|| {
                        eyre!(
                            "Unable to locate home directory.\
                            Please set `ZV_DIR` to use zv. If you think this is a bug please open an issue at <https://github.com/weezy20/zv/issues>"
                        )
                    })
            })?,
            false, /* Using fallback path */
        )
    };

    // Init ZV_DIR - create it if it doesn't exist
    match zv_dir.try_exists() {
        Ok(true) => {
            if !zv_dir.is_dir() {
                error(format!(
                    "zv directory exists but is not a directory: {}. Please check ZV_DIR env var. Aborting...",
                    zv_dir.display()
                ));
                bail!(eyre!("ZV_DIR exists but is not a directory"));
            }
        }
        Ok(false) => {
            if using_env {
                std::fs::create_dir_all(&zv_dir)
                    .map_err(ZvError::Io)
                    .wrap_err_with(|| {
                        format!(
                            "Error creating ZV_DIR from env var ZV_DIR={}",
                            std::env::var("ZV_DIR").expect("Handled in fetch_zv_dir()")
                        )
                    })?;
            } else {
                // Using fallback path $HOME/.zv (or $CWD/.zv in rare fallback)
                std::fs::create_dir(&zv_dir)
                    .map_err(ZvError::Io)
                    .wrap_err_with(|| {
                        format!("Failed to create default .zv at {}", zv_dir.display())
                    })?;
            }
        }
        Err(e) => {
            error(format!(
                "Failed to check zv directory at {:?}",
                zv_dir.display(),
            ));
            return Err(ZvError::Io(e).into());
        }
    };

    // Canonicalize the path before returning
    let zv_dir = std::fs::canonicalize(&zv_dir).map_err(ZvError::Io)?;

    Ok((zv_dir, using_env))
}

/// Print a warning message in yellow if stderr is a TTY
#[inline]
pub fn warn(message: impl Into<Cow<'static, str>>) {
    let msg = message.into();
    eprintln!("{}: {}", "Warning".yellow().bold(), msg);
}

/// Print an error message in red if stderr is a TTY
#[inline]
pub fn error(message: impl Into<Cow<'static, str>>) {
    let msg = message.into();
    eprintln!("{}: {}", "Error".red().bold(), msg);
}

/// Get the zig tarball name based on HOST arch-os
pub fn zig_tarball(version: ZigVersion) -> Option<String> {
    use target_lexicon::HOST;
    // Return None for Unknown variant
    let semver_version = match version {
        ZigVersion::Semver(v) => v,
        ZigVersion::Master(v) => v,
        ZigVersion::Stable(v) => v,
        ZigVersion::Latest(v) => v,
        ZigVersion::Unknown => return None,
    };

    let arch = match HOST.architecture {
        target_lexicon::Architecture::X86_64 => "x86_64",
        target_lexicon::Architecture::Aarch64(_) => "aarch64",
        target_lexicon::Architecture::X86_32(_) => "x86",
        target_lexicon::Architecture::Arm(_) => "arm",
        target_lexicon::Architecture::Riscv64(_) => "riscv64",
        target_lexicon::Architecture::Powerpc64 => "powerpc64",
        target_lexicon::Architecture::Powerpc64le => "powerpc64le",
        target_lexicon::Architecture::S390x => "s390x",
        target_lexicon::Architecture::LoongArch64 => "loongarch64",
        _ => return None,
    };

    let os = match HOST.operating_system {
        target_lexicon::OperatingSystem::Linux => "linux",
        target_lexicon::OperatingSystem::Darwin(_) => "macos",
        target_lexicon::OperatingSystem::Windows => "windows",
        target_lexicon::OperatingSystem::Freebsd => "freebsd",
        target_lexicon::OperatingSystem::Netbsd => "netbsd",
        _ => return None,
    };
    let ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.xz"
    };

    Some(format!("zig-{os}-{arch}-{semver_version}.{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zig_tarball_constructs_expected_filename() {
        let version =
            ZigVersion::Semver(semver::Version::parse("0.16.0-dev.65+ca2e17e0a").unwrap());
        let result = zig_tarball(version);

        // The exact result depends on the host architecture and OS
        // but should contain the version string and .tar.xz extension
        if let Some(tarball_name) = result {
            assert!(tarball_name.contains("0.16.0-dev.65+ca2e17e0a"));
            #[cfg(not(target_os = "windows"))]
            assert!(tarball_name.ends_with(".tar.xz"));
            #[cfg(target_os = "windows")]
            assert!(tarball_name.ends_with(".zip"));
            assert!(tarball_name.starts_with("zig-"));
        }
    }
}
