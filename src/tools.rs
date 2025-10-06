use crate::ZvError;
use color_eyre::{
    Result,
    eyre::{WrapErr, bail, eyre},
};
use std::{
    borrow::Cow,
    io,
    path::{Path, PathBuf},
};
use yansi::Paint;

/// Cross-platform canonicalize function that avoids UNC paths on Windows
pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Check if we're running in a TTY environment
#[inline]
pub(crate) fn is_tty() -> bool {
    yansi::is_enabled()
}

/// Check if the current environment supports interactive prompts
pub(crate) fn supports_interactive_prompts() -> bool {
    // Check basic TTY availability
    if !is_tty() {
        return false;
    }

    // Check for CI environments
    if std::env::var("CI").is_ok() {
        return false;
    }

    // Check for non-interactive terminals
    if let Ok(term) = std::env::var("TERM")
        && term == "dumb"
    {
        return false;
    }

    // Additional environment checks
    if std::env::var("DEBIAN_FRONTEND").as_deref() == Ok("noninteractive") {
        return false;
    }

    // For now, rely on yansi's TTY detection which handles most cases
    true
}

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
        (get_default_zv_dir()?, false /* Using fallback path */)
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
                // create_dir should be enough for default directory
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
    let zv_dir = canonicalize(&zv_dir).map_err(ZvError::Io)?;

    Ok((zv_dir, using_env))
}

/// Get the default ZV directory, handling emulated shells on Windows
pub(crate) fn get_default_zv_dir() -> Result<PathBuf> {
    // Use shell detection to determine appropriate home directory
    let shell = crate::shell::Shell::detect();

    if let Some(home_dir) = shell.get_home_dir() {
        Ok(home_dir.join(".zv"))
    } else {
        Err(eyre!(
            "Unable to locate home directory.\
            Please set `ZV_DIR` to use zv. If you think this is a bug please open an issue at <https://github.com/weezy20/zv/issues>"
        ))
    }
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

/// Calculate CRC32 hash of a file
pub fn calculate_file_hash(path: &Path) -> Result<u32> {
    use crc32fast::Hasher;
    use std::io::Read;

    let mut file = std::fs::File::open(path)
        .wrap_err_with(|| format!("Failed to open file for hashing: {}", path.display()))?;

    let mut hasher = Hasher::new();
    let mut buffer = [0; 8192]; // 8KB buffer

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .wrap_err_with(|| format!("Failed to read file for hashing: {}", path.display()))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize())
}

/// Compare file hashes to determine if files are identical
pub fn files_have_same_hash(path1: &Path, path2: &Path) -> Result<bool> {
    if !path1.exists() || !path2.exists() {
        return Ok(false);
    }

    Ok(calculate_file_hash(path1)? == calculate_file_hash(path2)?)
}
