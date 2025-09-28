use std::path::PathBuf;

use dialoguer::{
    Select,
    theme::{ColorfulTheme, Theme},
};
use yansi::Paint;

use super::{SetupContext, SetupRequirements};

/// Custom theme for zv interactive prompts that matches the existing color scheme
///
/// This theme provides consistent visual formatting that aligns with zv's brand colors:
/// - Zig orange (247, 147, 26) for active selections and headers
/// - Yellow for prompts and important information
/// - Green for success/positive actions
/// - Red for errors/abort actions  
/// - Cyan for file paths and informational text
/// - White/dim for inactive items and secondary text
#[derive(Debug, Clone)]
pub struct ZvTheme;

impl Theme for ZvTheme {
    /// Format the prompt text (the question being asked)
    fn format_prompt(&self, f: &mut dyn std::fmt::Write, prompt: &str) -> std::fmt::Result {
        write!(f, "{}", Paint::yellow(prompt).bold())
    }

    /// Format an individual item in a selection list
    fn format_select_prompt_item(
        &self,
        f: &mut dyn std::fmt::Write,
        text: &str,
        active: bool,
    ) -> std::fmt::Result {
        if active {
            // Active item: use zig orange background with white text for visibility
            write!(
                f,
                "{}",
                Paint::new(format!("❯ {}", text))
                    .fg(yansi::Color::White)
                    .bg(yansi::Color::Rgb(247, 147, 26))
            )
        } else {
            // Inactive items: use dim white for subtle appearance
            write!(f, "  {}", Paint::new(text).fg(yansi::Color::White).dim())
        }
    }

    /// Format the confirmation prompt (y/n style prompts)
    fn format_confirm_prompt(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> std::fmt::Result {
        match default {
            Some(true) => write!(
                f,
                "{} {}",
                Paint::yellow(prompt).bold(),
                Paint::dim("[Y/n]")
            ),
            Some(false) => write!(
                f,
                "{} {}",
                Paint::yellow(prompt).bold(),
                Paint::dim("[y/N]")
            ),
            None => write!(
                f,
                "{} {}",
                Paint::yellow(prompt).bold(),
                Paint::dim("[y/n]")
            ),
        }
    }

    /// Format confirmation prompt after user makes a selection
    fn format_confirm_prompt_selection(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> std::fmt::Result {
        let selection_text = match selection {
            Some(true) => Paint::green("yes"),
            Some(false) => Paint::red("no"),
            None => Paint::dim("n/a"),
        };
        write!(f, "{} {}", Paint::yellow(prompt).bold(), selection_text)
    }

    /// Format the final selection result for select prompts
    fn format_select_prompt_selection(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> std::fmt::Result {
        write!(f, "{} {}", Paint::yellow(prompt).bold(), Paint::cyan(sel))
    }

    /// Format input prompts (text input)
    fn format_input_prompt(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        default: Option<&str>,
    ) -> std::fmt::Result {
        match default {
            Some(default) => write!(
                f,
                "{} {}",
                Paint::yellow(prompt).bold(),
                Paint::dim(&format!("[{}]", default))
            ),
            None => write!(f, "{}", Paint::yellow(prompt).bold()),
        }
    }

    /// Format input prompt after user provides input
    fn format_input_prompt_selection(
        &self,
        f: &mut dyn std::fmt::Write,
        prompt: &str,
        sel: &str,
    ) -> std::fmt::Result {
        write!(f, "{} {}", Paint::yellow(prompt).bold(), Paint::cyan(sel))
    }
}

impl ZvTheme {
    /// Create a new instance of the ZV theme
    pub fn new() -> Self {
        Self
    }
}

impl Default for ZvTheme {
    fn default() -> Self {
        Self::new()
    }
}

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

        // Prompt for PATH choice
        let path_choice = self.prompt_path_choice()?;

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
            super::ZvDirAction::MakePermanent { current_path: _ } => {
                // Always default to skipping ZV_DIR permanent setup for consistency
                Ok(ZvDirChoice::Skip)
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

        // Display explanation header with enhanced visual formatting
        println!();
        println!(
            "{}",
            Paint::new("━━━ ZV_DIR Configuration ━━━")
                .fg(yansi::Color::Rgb(247, 147, 26))
                .bold()
        );
        println!();
        println!("Your environment has ZV_DIR set to a custom location:");
        println!(
            "  {} {}",
            Paint::new("Current detected:").bold(),
            Paint::cyan(&current_path.display()).underline()
        );
        println!(
            "  {} {}",
            Paint::new("Default location:").bold(),
            Paint::dim(&default_path.display())
        );
        println!();

        // Always recommend skipping ZV_DIR permanent setup and using default location
        let enhanced_options = vec![
            format!(
                "Make detected ZV_DIR permanent → {}",
                Paint::cyan(&current_path.display()).bold()
            ),
            format!(
                "Make default ZV_DIR permanent → {}",
                Paint::dim(&default_path.display())
            ),
            format!(
                "Skip making ZV_DIR a permanent environment variable {}",
                Paint::green("(recommended)")
            ),
        ];
        let default_index = 2; // Always default to skipping ZV_DIR permanent setup

        // Create themed prompt using custom zv theme
        let theme = ZvTheme::new();
        let selection = Select::with_theme(&theme)
            .with_prompt("How would you like to handle ZV_DIR?")
            .items(&enhanced_options)
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
                    Paint::new("✓ Will make detected ZV_DIR a permanent environment variable")
                        .fg(yansi::Color::Green)
                        .bold()
                );
                Ok(ZvDirChoice::UseDetected(current_path))
            }
            1 => {
                println!();
                println!(
                    "{}",
                    Paint::new("✓ Will make default ZV_DIR a permanent environment variable")
                        .fg(yansi::Color::Green)
                        .bold()
                );
                Ok(ZvDirChoice::UseDefault(default_path))
            }
            2 => {
                println!();
                println!(
                    "{}",
                    Paint::new("⚠ ZV_DIR will not be made a permanent environment variable")
                        .fg(yansi::Color::Yellow)
                        .bold()
                );
                println!(
                    "  {} You'll need to set ZV_DIR manually in future sessions",
                    Paint::dim("Note:")
                );
                Ok(ZvDirChoice::Skip)
            }
            _ => unreachable!("Invalid selection index"),
        }
    }

    /// Prompt user for PATH modification choice with interactive dialog
    fn prompt_path_choice(&self) -> crate::Result<PathChoice> {
        // If PATH is already configured, no need to prompt
        if matches!(
            self.requirements.path_action,
            super::PathAction::AlreadyConfigured
        ) {
            return Ok(PathChoice::Proceed(self.context.app.bin_path().clone()));
        }

        // Get the bin path that will be added to PATH
        let bin_path = match &self.requirements.path_action {
            super::PathAction::AddToRegistry { bin_path } => bin_path,
            super::PathAction::GenerateEnvFile { bin_path, .. } => bin_path,
            super::PathAction::AlreadyConfigured => {
                // This case is handled above, but include for completeness
                return Ok(PathChoice::Proceed(self.context.app.bin_path().clone()));
            }
        };

        // Display explanation header with enhanced visual formatting
        println!();
        println!(
            "{}",
            Paint::new("━━━ PATH Configuration ━━━")
                .fg(yansi::Color::Rgb(247, 147, 26))
                .bold()
        );
        println!();
        println!("zv needs to add its binary directory to your PATH to function properly:");
        println!(
            "  {} {}",
            Paint::new("Will add:").bold(),
            Paint::cyan(&bin_path.display()).underline()
        );

        // Show platform-specific modification method with enhanced formatting
        if self.context.shell.is_windows_shell() && !self.context.shell.is_powershell_in_unix() {
            println!(
                "  {} {}",
                Paint::new("Method:").bold(),
                Paint::dim("Windows system environment variables")
            );
            println!(
                "  {} {}",
                Paint::new("Scope:").bold(),
                Paint::dim("Current user registry")
            );
        } else {
            println!(
                "  {} {}",
                Paint::new("Method:").bold(),
                Paint::dim("Shell profile modification")
            );
            match &self.requirements.path_action {
                super::PathAction::GenerateEnvFile { rc_file, .. } => {
                    println!(
                        "  {} {}",
                        Paint::new("Profile:").bold(),
                        Paint::dim(&rc_file.display())
                    );
                }
                _ => {}
            }
        }

        println!();
        println!("{}", Paint::new("ℹ Note:").fg(yansi::Color::Blue).bold());
        println!("  PATH modification is required for zv/zig/zls to work from any directory.");
        println!();

        // Create options with enhanced formatting and default indicator
        let options = vec![
            format!(
                "Proceed → Add {} to PATH {}",
                Paint::cyan(&bin_path.display()).bold(),
                Paint::green("(recommended)")
            ),
            format!("{}", Paint::red("Abort setup")),
        ];

        // Create themed prompt with proceed as default (index 0)
        let theme = ZvTheme::new();
        let selection = Select::with_theme(&theme)
            .with_prompt("PATH modification is required for zv to function:")
            .items(&options)
            .default(0) // Proceed is the default option
            .interact()
            .map_err(|e| {
                crate::ZvError::shell_setup_failed(
                    "interactive-prompt",
                    &format!("Failed to get user input for PATH choice: {}", e),
                )
            })?;

        // Convert selection to PathChoice
        match selection {
            0 => {
                println!();
                println!(
                    "{}",
                    Paint::new("✓ Proceeding with PATH modifications")
                        .fg(yansi::Color::Green)
                        .bold()
                );
                Ok(PathChoice::Proceed(bin_path.clone()))
            }
            1 => {
                println!();
                println!(
                    "{}",
                    Paint::new("✗ Setup aborted by user")
                        .fg(yansi::Color::Red)
                        .bold()
                );
                println!(
                    "  {} zv may not function properly without PATH configuration",
                    Paint::dim("Note:")
                );
                Ok(PathChoice::Abort)
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
