use super::instructions::ModifiedFile;
use crate::app::App;
use crate::shell::Shell;

/// Core context for setup operations containing all information needed for setup
#[derive(Debug, Clone)]
pub struct SetupContext {
    /// Enhanced shell information with context
    pub shell: Shell,
    /// Application state and paths
    pub app: App,
    /// Whether custom ZV_DIR is being used
    pub using_env_var: bool,
    /// Whether to perform actual operations or just preview
    pub dry_run: bool,
    /// Whether to disable interactive prompts and use defaults
    pub no_interactive: bool,
    /// Files modified during setup (for post-setup instructions)
    /// Uses Arc<Mutex<>> to allow modification through immutable references
    /// since setup functions take &SetupContext but need to track modifications
    pub modified_files: std::sync::Arc<std::sync::Mutex<Vec<ModifiedFile>>>,
}

impl SetupContext {
    /// Create a new setup context
    pub fn new(shell: Shell, app: App, using_env_var: bool, dry_run: bool) -> Self {
        Self {
            shell,
            app,
            using_env_var,
            dry_run,
            no_interactive: false,
            modified_files: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Create a new setup context with interactive mode control
    pub fn new_with_interactive(
        shell: Shell,
        app: App,
        using_env_var: bool,
        dry_run: bool,
        no_interactive: bool,
    ) -> Self {
        Self {
            shell,
            app,
            using_env_var,
            dry_run,
            no_interactive,
            modified_files: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Add a modified file to the context
    pub fn add_modified_file(&self, modified_file: ModifiedFile) {
        if let Ok(mut files) = self.modified_files.lock() {
            files.push(modified_file);
        }
    }

    /// Get a copy of all modified files
    pub fn get_modified_files(&self) -> Vec<ModifiedFile> {
        self.modified_files
            .lock()
            .map(|files| files.clone())
            .unwrap_or_default()
    }
}
