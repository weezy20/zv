use crate::shell::setup::{SetupContext, execute_setup, post_setup_actions, pre_setup_checks};
use crate::{App, Shell, ZigVersion};
use color_eyre::eyre::Context as _;
use yansi::Paint;

/// Main setup_shell function that orchestrates the three-phase setup process
/// This is the public interface that maintains backward compatibility

pub async fn setup_shell(
    app: &mut App,
    using_env_var: bool,
    dry_run: bool,
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
        let context = SetupContext::new(
            app.shell.clone().unwrap_or_default(),
            app.clone(),
            using_env_var,
            dry_run,
        );
        post_setup_actions(&context).await?;
        return Ok(());
    }

    // App::init() for zv_main() ensures shell is always here
    // but in the rare case, fallback to default which calls Shell::detect()
    let shell = app.shell.clone().unwrap_or_default();

    // Create setup context
    let context = SetupContext::new(shell, app.clone(), using_env_var, dry_run);

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

    // Phase 2: Execute setup
    execute_setup(&context, &requirements)
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
