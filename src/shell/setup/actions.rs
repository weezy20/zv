use std::path::PathBuf;

/// Actions for ZV_DIR environment variable setup
#[derive(Debug, Clone)]
pub enum ZvDirAction {
    /// ZV_DIR is not set, no action needed
    NotSet,
    /// ZV_DIR is set temporarily, ask user to make permanent
    MakePermanent { current_path: PathBuf },
    /// ZV_DIR is already permanent, no action needed
    AlreadyPermanent,
}

/// Actions for PATH configuration across platforms
#[derive(Debug, Clone)]
pub enum PathAction {
    /// PATH already contains zv bin directory
    AlreadyConfigured,
    /// Need to add zv bin to PATH via registry (Windows native shells)
    AddToRegistry { bin_path: PathBuf },
    /// Need to generate env file and modify RC files (Unix shells)
    GenerateEnvFile {
        env_file_path: PathBuf,
        rc_file: PathBuf,
        bin_path: PathBuf,
    },
}

impl ZvDirAction {
    /// Check if this action requires user interaction
    pub fn requires_user_interaction(&self) -> bool {
        matches!(self, ZvDirAction::MakePermanent { .. })
    }

    /// Check if this action will modify the system
    pub fn modifies_system(&self) -> bool {
        matches!(self, ZvDirAction::MakePermanent { .. })
    }
}

impl PathAction {
    /// Check if this action will modify the system
    pub fn modifies_system(&self) -> bool {
        !matches!(self, PathAction::AlreadyConfigured)
    }

    /// Get the bin path that will be added to PATH
    pub fn bin_path(&self) -> Option<&PathBuf> {
        match self {
            PathAction::AlreadyConfigured => None,
            PathAction::AddToRegistry { bin_path } => Some(bin_path),
            PathAction::GenerateEnvFile { bin_path, .. } => Some(bin_path),
        }
    }
}
