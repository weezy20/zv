use crate::shell::setup::{
    SetupContext, execute_setup, post_setup_actions, pre_setup_checks,
    InteractiveSetup, apply_user_choices, handle_interactive_error, is_recoverable_interactive_error
};
use crate::{App, Shell, ZigVersion};
use color_eyre::eyre::Context as _;
use yansi::Paint;

/// Main setup_shell function that orchestrates the three-phase setup process
/// This is the public interface that maintains backward compatibility and supports interactive mode

pub async fn setup_shell(
    app: &mut App,
    using_env_var: bool,
    dry_run: bool,
    no_interactive: bool,
    default_version: Option<ZigVersion>,
) -> crate::Result<()> {
    // Check if shell environment is already set up
    if app.source_set {
        println!(
            "{}",
            Paint::white("✓ Shell environment PATH already includes path to zv")
        );

        // Even when shell environment is set up, we need to check if binary needs updating
        // or if shims need regeneration
        let context = SetupContext::new_with_interactive(
            app.shell.clone().unwrap_or_default(),
            app.clone(),
            using_env_var,
            dry_run,
            no_interactive,
        );
        post_setup_actions(&context).await?;
        return Ok(());
    }

    // App::init() for zv_main() ensures shell is always here
    // but in the rare case, fallback to default which calls Shell::detect()
    let shell = app.shell.clone().unwrap_or_default();

    // Create setup context with interactive mode control
    let context = SetupContext::new_with_interactive(
        shell,
        app.clone(),
        using_env_var,
        dry_run,
        no_interactive,
    );

    if dry_run {
        println!(
            "{} zv setup for {} shell...",
            Paint::yellow("Previewing"),
            Paint::cyan(&context.shell.to_string())
        );
    } else {
        println!(
            "Setting up zv for {} shell...",
            Paint::cyan(&context.shell.to_string())
        );
    }

    // Phase 1: Pre-setup checks
    let requirements = pre_setup_checks(&context)
        .await
        .with_context(|| "Pre-setup checks failed")?;

    // Phase 2: Interactive confirmation (default behavior) or fallback to existing behavior
    let final_requirements = if should_use_interactive(&context) {
        let interactive_setup = InteractiveSetup::new(context.clone(), requirements.clone());
        
        match interactive_setup.run_interactive_flow().await {
            Ok(user_choices) => {
                // Interactive flow succeeded, apply user choices
                apply_user_choices(requirements, user_choices)?
            }
            Err(e) => {
                // Try to downcast the error to ZvError for better handling
                if let Some(zv_error) = e.downcast_ref::<crate::ZvError>() {
                    // Interactive flow failed, check if we can recover
                    if is_recoverable_interactive_error(zv_error) {
                        // Provide clear error message and fallback
                        if let Some(message) = handle_interactive_error(zv_error) {
                            crate::tools::warn(message);
                            crate::tools::warn("Falling back to non-interactive mode");
                        }
                        requirements
                    } else {
                        // User explicitly cancelled or non-recoverable error
                        if let Some(suggestion) = handle_interactive_error(zv_error) {
                            crate::tools::error(suggestion);
                        }
                        return Err(e);
                    }
                } else {
                    // Non-ZvError, don't attempt recovery
                    return Err(e);
                }
            }
        }
    } else {
        // Fallback to existing behavior
        requirements
    };

    // Phase 3: Execute setup based on final requirements
    execute_setup(&context, &final_requirements)
        .await
        .with_context(|| "Setup execution failed")?;

    // Success message
    if dry_run {
        println!("{}", Paint::cyan("→ Dry Run Complete"));
        println!("Run {} to apply these changes", Paint::green("zv setup"));
    } else {
        println!("{}", Paint::green("→ Setup Complete"));
        println!(
            "Restart your shell or run the appropriate source command to apply changes immediately"
        );
    }

    Ok(())
}

/// Determine if interactive mode should be used based on context and environment
/// 
/// Interactive mode is automatically disabled when:
/// - `--no-interactive` flag is provided
/// - CI environment is detected (CI environment variable is set)
/// - TERM environment variable is set to "dumb"
/// - TTY is not available for interactive prompts
fn should_use_interactive(context: &SetupContext) -> bool {
    // Don't use interactive mode if explicitly disabled
    if context.no_interactive {
        return false;
    }

    // Don't use interactive mode in CI environments
    if std::env::var("CI").is_ok() {
        return false;
    }

    // Don't use interactive mode if TERM is dumb
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
    }

    // Check if TTY is available for interactive prompts
    crate::tools::supports_interactive_prompts()
}
