//! Global types used across zv

pub mod error;
pub use error::*;

mod zig;
mod zigversion;

pub use zig::*;
pub use zigversion::*;

#[derive(Debug, Clone, Default)]
/// Application configuration provided by frontend
pub struct UserConfig {
    pub zv_base_path: std::path::PathBuf,
    pub shell: Option<crate::Shell>,
}
