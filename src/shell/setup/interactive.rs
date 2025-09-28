use std::path::PathBuf;

use dialoguer::{Select, theme::ColorfulTheme};
use yansi::Paint;

use super::{SetupContext, SetupRequirements};

/// Core interactive setup coordinator
#[derive(Debug, Clone)]
pub struct InteractiveSetup {
    context: SetupContext,
    requirements: SetupRequirements,
}

/// User choices collected from interactive prompts
#[derive(Debug, Clone)]
pub struct UserChoices {
    pub zv_dir_choice: ZvDirChoice,
    pub path_choice: PathChoice,
    pub confirmed: bool,
}

/// User choice for ZV_DIR handling
#[derive(Debug, Clone)]
pub enum ZvDirChoice {
    /// Use the currently detected ZV_DIR path
    UseDetected(PathBuf),
    /// Use the default ZV_DIR path
    UseDefault(PathBuf),
    /// Skip making ZV_DIR permanent
    Skip,
}

/// User choice for PATH modification
#[derive(Debug, Clone)]
pub enum PathChoice {
    /// Proceed with adding the specified path to PATH
    Proceed(PathBuf),
    /// Abort the setup process
    Abort,
}

impl InteractiveSetup {
    /// Create a new interactive setup instance
    pub fn new(context: SetupContext, requirements: SetupRequirements) -> Self {
        Self {
            context,
            requirements,
        }
    }

    /// Run the interactive flow and collect user choices
    pub async fn run_interactive_flow(&self) -> crate::Result<UserChoices> {
        // Prompt for ZV_DIR choice
        let zv_dir_choice = self.prompt_zv_dir_choice()?;

        // Prompt for PATH choice (placeholder for now)
        let path_choice = self.get_default_path_choice();

        Ok(UserChoices {
            zv_dir_choice,
            path_choice,
            confirmed: true,
        })
    }

    /// Check if interactive mode should be used
    pub fn should_use_interactive(&self) -> bool {
        // Check if we're in a TTY environment and interactive mode is not disabled
        !self.context.no_interactive && self.is_tty_available()
    }

    /// Check if TTY is available for interactive prompts
    fn is_tty_available(&self) -> bool {
        // Use the existing TTY detection from tools module
        crate::tools::is_tty()
    }

    /// Get default ZV_DIR choice based on platform and current state
    fn get_default_zv_dir_choice(&self) -> crate::Result<ZvDirChoice> {
        match &self.requirements.zv_dir_action {
            super::ZvDirAction::NotSet => {
                let default_path = self.get_default_zv_dir_path()?;
                Ok(ZvDirChoice::UseDefault(default_path))
            }
            super::ZvDirAction::MakePermanent { current_path } => {
                // On Unix systems, default to skip; on Windows, default to detected
                if self.context.shell.is_windows_shell()
                    && !self.context.shell.is_powershell_in_unix()
                {
                    Ok(ZvDirChoice::UseDetected(current_path.clone()))
                } else {
                    Ok(ZvDirChoice::Skip)
                }
            }
            super::ZvDirAction::AlreadyPermanent => {
                Ok(ZvDirChoice::UseDetected(self.context.app.path().clone()))
            }
        }
    }

    /// Get default PATH choice
    fn get_default_path_choice(&self) -> PathChoice {
        match &self.requirements.path_action {
            super::PathAction::AlreadyConfigured => {
                // This shouldn't happen in interactive flow, but handle gracefully
                PathChoice::Proceed(self.context.app.bin_path().clone())
            }
            super::PathAction::AddToRegistry { bin_path } => PathChoice::Proceed(bin_path.clone()),
            super::PathAction::GenerateEnvFile { bin_path, .. } => {
                PathChoice::Proceed(bin_path.clone())
            }
        }
    }

    /// Get the default ZV_DIR path, returning an error if it cannot be determined
    fn get_default_zv_dir_path(&self) -> crate::Result<PathBuf> {
        crate::tools::get_default_zv_dir()
    }

    /// Prompt user for ZV_DIR choice with interactive dialog
    fn prompt_zv_dir_choice(&self) -> crate::Result<ZvDirChoice> {
        // Only show ZV_DIR prompt if we're using an environment variable
        if !self.context.using_env_var {
            let default_path = self.get_default_zv_dir_path()?;
            return Ok(ZvDirChoice::UseDefault(default_path));
        }

        // If ZV_DIR is already permanent, no need to prompt
        if matches!(
            self.requirements.zv_dir_action,
            super::ZvDirAction::AlreadyPermanent
        ) {
            return Ok(ZvDirChoice::UseDetected(self.context.app.path().clone()));
        }

        // Get current and default paths
        let current_path = self.context.app.path().clone();
        let default_path = self.get_default_zv_dir_path()?;

        // If current path is the same as default, no need to prompt
        if current_path == default_path {
            return Ok(ZvDirChoice::UseDefault(default_path));
        }

        // Display explanation header
        println!();
        println!("{}", Paint::yellow("ZV_DIR Configuration").bold());
        println!();
        println!("Your environment has ZV_DIR set to a custom location:");
        println!(
            "  Current detected: {}",
            Paint::cyan(&current_path.display())
        );
        println!(
            "  Default location: {}",
            Paint::dim(&default_path.display())
        );
        println!();

        // Create options with bullet-point style formatting
        let options = vec![
            format!("• Use detected ZV_DIR ({})", current_path.display()),
            format!("• Use default ZV_DIR ({})", default_path.display()),
            "• Skip making ZV_DIR permanent".to_string(),
        ];

        // Platform-specific default selection
        // Unix defaults to skip (index 2), Windows defaults to detected (index 0)
        let default_index = if self.context.shell.is_windows_shell()
            && !self.context.shell.is_powershell_in_unix()
        {
            0 // Windows defaults to using detected ZV_DIR
        } else {
            2 // Unix defaults to skip making ZV_DIR permanent
        };

        // Create themed prompt
        let theme = ColorfulTheme::default();
        let selection = Select::with_theme(&theme)
            .with_prompt("How would you like to handle ZV_DIR?")
            .items(&options)
            .default(default_index)
            .interact()
            .map_err(|e| {
                crate::ZvError::shell_setup_failed(
                    "interactive-prompt",
                    &format!("Failed to get user input for ZV_DIR choice: {}", e),
                )
            })?;

        // Convert selection to ZvDirChoice
        match selection {
            0 => {
                println!();
                println!(
                    "{}",
                    Paint::green("Will make detected ZV_DIR permanent in environment")
                );
                Ok(ZvDirChoice::UseDetected(current_path))
            }
            1 => {
                println!();
                println!("{}", Paint::green("Will use default ZV_DIR location"));
                Ok(ZvDirChoice::UseDefault(default_path))
            }
            2 => {
                println!();
                println!("{}", Paint::yellow("ZV_DIR will not be made permanent"));
                println!("  Note: You'll need to set ZV_DIR manually in future sessions");
                Ok(ZvDirChoice::Skip)
            }
            _ => unreachable!("Invalid selection index"),
        }
    }
}

impl UserChoices {
    /// Create new user choices with default values
    pub fn new(zv_dir_choice: ZvDirChoice, path_choice: PathChoice) -> Self {
        Self {
            zv_dir_choice,
            path_choice,
            confirmed: true,
        }
    }

    /// Check if the user confirmed the setup
    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    /// Check if the user chose to abort
    pub fn should_abort(&self) -> bool {
        matches!(self.path_choice, PathChoice::Abort) || !self.confirmed
    }
}

impl ZvDirChoice {
    /// Get the path associated with this choice, if any
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            ZvDirChoice::UseDetected(path) => Some(path),
            ZvDirChoice::UseDefault(path) => Some(path),
            ZvDirChoice::Skip => None,
        }
    }

    /// Check if this choice requires making ZV_DIR permanent
    pub fn requires_permanent_setting(&self) -> bool {
        matches!(self, ZvDirChoice::UseDetected(_))
    }
}

impl PathChoice {
    /// Get the path that will be added to PATH
    pub fn path(&self) -> Option<&PathBuf> {
        match self {
            PathChoice::Proceed(path) => Some(path),
            PathChoice::Abort => None,
        }
    }
}

/// Apply user choices to modify setup requirements
pub fn apply_user_choices(
    requirements: SetupRequirements,
    user_choices: UserChoices,
) -> crate::Result<SetupRequirements> {
    // If user chose to abort, return an error
    if user_choices.should_abort() {
        return Err(crate::ZvError::shell_setup_failed(
            "user-abort",
            "Setup was aborted by user choice",
        )
        .into());
    }

    let mut modified_requirements = requirements;

    // Apply ZV_DIR choice
    modified_requirements.zv_dir_action = match user_choices.zv_dir_choice {
        ZvDirChoice::UseDetected(path) => super::ZvDirAction::MakePermanent { current_path: path },
        ZvDirChoice::UseDefault(_) => super::ZvDirAction::NotSet,
        ZvDirChoice::Skip => super::ZvDirAction::NotSet,
    };

    // Apply PATH choice
    if let Some(bin_path) = user_choices.path_choice.path() {
        // Keep the existing path action but ensure it uses the correct bin path
        modified_requirements.path_action = match modified_requirements.path_action {
            super::PathAction::AlreadyConfigured => super::PathAction::AlreadyConfigured,
            super::PathAction::AddToRegistry { .. } => super::PathAction::AddToRegistry {
                bin_path: bin_path.clone(),
            },
            super::PathAction::GenerateEnvFile {
                env_file_path,
                rc_file,
                ..
            } => super::PathAction::GenerateEnvFile {
                env_file_path,
                rc_file,
                bin_path: bin_path.clone(),
            },
        };
    }

    // Update needs_post_setup based on modified actions
    modified_requirements.needs_post_setup = !modified_requirements.zv_bin_in_path
        || modified_requirements.zv_dir_action.modifies_system()
        || modified_requirements.path_action.modifies_system();

    Ok(modified_requirements)
}
