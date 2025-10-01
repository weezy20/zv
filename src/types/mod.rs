//! Global types used across zv

pub mod error;
pub use error::*;

mod target_triple;
mod zigversion;

pub use target_triple::*;
pub use zigversion::*;

use color_eyre::eyre::eyre;

#[derive(Debug, Clone, Default)]
/// Application configuration provided by frontend
pub struct UserConfig {
    pub zv_base_path: std::path::PathBuf,
    pub shell: Option<crate::Shell>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ArchiveExt {
    #[default]
    TarXz,
    Zip,
}

impl std::str::FromStr for ArchiveExt {
    type Err = ZvError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tar.xz" => Ok(ArchiveExt::TarXz),
            "zip" => Ok(ArchiveExt::Zip),
            _ => Err(eyre!("Unsupported archive extension: {s}").into()),
        }
    }
}

impl std::fmt::Display for ArchiveExt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArchiveExt::TarXz => write!(f, "tar.xz"),
            ArchiveExt::Zip => write!(f, "zip"),
        }
    }
}

/// Enum representing the type of shim to detect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shim {
    Zv,
    Zig,
    Zls,
}

impl Shim {
    /// Returns the executable name for this shim
    pub fn executable_name(&self) -> &'static str {
        match self {
            Shim::Zv => {
                if cfg!(target_os = "windows") {
                    "zv.exe"
                } else {
                    "zv"
                }
            }
            Shim::Zig => {
                if cfg!(target_os = "windows") {
                    "zig.exe"
                } else {
                    "zig"
                }
            }
            Shim::Zls => {
                if cfg!(target_os = "windows") {
                    "zls.exe"
                } else {
                    "zls"
                }
            }
        }
    }
}
