use super::actions::{PathAction, ZvDirAction};

/// Requirements determined by pre-setup analysis
#[derive(Debug, Clone)]
pub struct SetupRequirements {
    /// Whether zv bin directory is already in PATH
    pub zv_bin_in_path: bool,
    /// What action is needed for ZV_DIR environment variable
    pub zv_dir_action: ZvDirAction,
    /// What action is needed for PATH configuration
    pub path_action: PathAction,
    /// Whether post-setup actions are required
    pub needs_post_setup: bool,
}

impl SetupRequirements {
    /// Create new setup requirements
    pub fn new(
        zv_bin_in_path: bool,
        zv_dir_action: ZvDirAction,
        path_action: PathAction,
        needs_post_setup: bool,
    ) -> Self {
        Self {
            zv_bin_in_path,
            zv_dir_action,
            path_action,
            needs_post_setup,
        }
    }
}
