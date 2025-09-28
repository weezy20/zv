use crate::shell::setup::{
    SetupContext, execute_setup, post_setup_actions, pre_setup_checks,
    InteractiveSetup, apply_user_choices
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
        let user_choices = interactive_setup.run_interactive_flow().await?;
        apply_user_choices(requirements, user_choices)?
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
fn should_use_interactive(context: &SetupContext) -> bool {
    // Don't use interactive mode if explicitly disabled
    if context.no_interactive {
        return false;
    }

    // Don't use interactive mode in dry run
    if context.dry_run {
        return false;
    }

    // Check if TTY is available for interactive prompts
    crate::tools::is_tty()
}
