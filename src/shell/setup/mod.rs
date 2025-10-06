use color_eyre::eyre::Context;
use yansi::Paint;

pub mod actions;
pub mod context;
pub mod instructions;
pub mod interactive;
pub mod requirements;
pub mod unix;
pub mod windows;

pub use actions::*;
pub use context::*;
pub use instructions::*;
pub use interactive::*;
pub use requirements::*;

/// Pre-setup checks phase - analyze current system state and determine required actions
pub async fn pre_setup_checks(context: &SetupContext) -> crate::Result<SetupRequirements> {
    let bin_path_in_path = check_bin_path_in_path(context);
    let zv_dir_action = determine_zv_dir_action(context).await?;
    let path_action = determine_path_action(context, bin_path_in_path);

    let needs_post_setup = !bin_path_in_path
        || matches!(zv_dir_action, ZvDirAction::MakePermanent { .. })
        || !matches!(path_action, PathAction::AlreadyConfigured);

    Ok(SetupRequirements::new(
        bin_path_in_path,
        zv_dir_action,
        path_action,
        needs_post_setup,
    ))
}

/// Check if zv bin directory is already in PATH
pub fn check_bin_path_in_path(context: &SetupContext) -> bool {
    use crate::shell::path_utils::check_dir_in_path_for_shell;
    check_dir_in_path_for_shell(&context.shell, context.app.bin_path())
}

/// Determine what action is needed for ZV_DIR environment variable
pub async fn determine_zv_dir_action(context: &SetupContext) -> crate::Result<ZvDirAction> {
    if !context.using_env_var {
        // Using default path, no action needed for ZV_DIR
        return Ok(ZvDirAction::NotSet);
    }

    let zv_dir = context.app.path();

    // Check if ZV_DIR is already set permanently
    let is_permanent = if cfg!(windows) {
        #[cfg(windows)]
        {
            windows::check_zv_dir_permanent_windows(zv_dir).await?
        }
        #[cfg(not(windows))]
        {
            false
        } // This branch should never be reached due to cfg!(windows) check above
    } else {
        unix::check_zv_dir_permanent_unix(&context.shell, zv_dir).await?
    };

    if is_permanent {
        Ok(ZvDirAction::AlreadyPermanent)
    } else {
        // ZV_DIR is set temporarily, determine action based on mode
        if context.dry_run {
            // In dry run, assume we would make it permanent for preview
            Ok(ZvDirAction::MakePermanent {
                current_path: zv_dir.clone(),
            })
        } else if will_use_interactive_mode(context) {
            // Interactive mode will handle the user choice, so return MakePermanent
            // as a placeholder - the interactive flow will determine the actual choice
            Ok(ZvDirAction::MakePermanent {
                current_path: zv_dir.clone(),
            })
        } else {
            // Non-interactive mode: ask user with old prompt
            let should_make_permanent = ask_user_zv_dir_confirmation(zv_dir)?;
            if should_make_permanent {
                Ok(ZvDirAction::MakePermanent {
                    current_path: zv_dir.clone(),
                })
            } else {
                Ok(ZvDirAction::NotSet)
            }
        }
    }
}

/// Check if interactive mode will be used based on context
fn will_use_interactive_mode(context: &SetupContext) -> bool {
    // Don't use interactive mode if explicitly disabled
    if context.no_interactive {
        return false;
    }

    // Don't use interactive mode in CI environments
    if std::env::var("CI").is_ok() {
        return false;
    }

    // Don't use interactive mode if TERM is dumb
    if let Ok(term) = std::env::var("TERM")
        && term == "dumb"
    {
        return false;
    }

    // Check if TTY is available for interactive prompts
    crate::tools::supports_interactive_prompts()
}

/// Determine what action is needed for PATH configuration
pub fn determine_path_action(context: &SetupContext, bin_path_in_path: bool) -> PathAction {
    if bin_path_in_path {
        return PathAction::AlreadyConfigured;
    }

    let bin_path = context.app.bin_path().clone();

    if context.shell.is_windows_shell() && !context.shell.is_powershell_in_unix() {
        // Windows native shells use registry
        PathAction::AddToRegistry { bin_path }
    } else {
        // Unix shells (including Unix shells on Windows) use env files
        let env_file_path = context.app.env_path().clone();
        let rc_file = unix::select_rc_file(&context.shell);

        PathAction::GenerateEnvFile {
            env_file_path,
            rc_file,
            bin_path,
        }
    }
}

/// Ask user for confirmation to make ZV_DIR permanent
fn ask_user_zv_dir_confirmation(zv_dir: &std::path::Path) -> crate::Result<bool> {
    use std::io::{self, Write};
    use yansi::Paint;

    let home_dir = dirs::home_dir().ok_or_else(|| {
        crate::ZvError::shell_context_creation_failed("Could not determine home directory")
    })?;
    let default_zv_dir = home_dir.join(".zv");

    // Show info about custom ZV_DIR
    println!("{}\n", Paint::yellow("⚠ Custom ZV_DIR detected").bold());
    println!(
        "Your environment has ZV_DIR set to path: {}",
        Paint::cyan(&zv_dir.display().to_string())
    );
    println!(
        "Default path would be: {}",
        Paint::dim(&default_zv_dir.display().to_string())
    );
    println!();

    // Determine the target for permanent setting
    let target_description = if cfg!(windows) {
        "system environment variables"
    } else {
        ".profile"
    };

    // Offer to set ZV_DIR permanently
    print!(
        "Do you want zv to make ZV_DIR={} permanent by adding it to {}? [y/N]: ",
        Paint::cyan(&zv_dir.display().to_string()),
        Paint::green(&target_description)
    );
    io::stdout().flush().map_err(|e| {
        crate::ZvError::shell_setup_failed(
            "user-confirmation",
            &format!("Failed to flush stdout: {}", e),
        )
    })?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(|e| {
        crate::ZvError::shell_setup_failed(
            "user-confirmation",
            &format!("Failed to read user input: {}", e),
        )
    })?;

    let response = input.trim().to_lowercase();
    let should_set_permanent = matches!(response.as_str(), "y" | "yes");

    if !should_set_permanent {
        // User chose not to set permanently, show warnings
        println!();
        println!("{}", Paint::yellow("⚠ Important considerations:"));
        println!(
            "• Temporary ZV_DIR settings will break zv in new sessions unless the next session also has it set"
        );
        println!("• Ensure ZV_DIR is permanently set in your shell profile or system environment");
        println!();
    } else {
        println!();
        println!(
            "{}",
            Paint::green("zv will set ZV_DIR permanently during setup...")
        );
        println!();
    }

    Ok(should_set_permanent)
}

/// Execute ZV_DIR setup based on the determined action
pub async fn execute_zv_dir_setup(
    context: &SetupContext,
    action: &ZvDirAction,
) -> crate::Result<()> {
    match action {
        ZvDirAction::NotSet => {
            // No action needed
            if !context.dry_run {
                println!("ZV_DIR: Using default path (no permanent setting needed)");
            }
            Ok(())
        }
        ZvDirAction::AlreadyPermanent => {
            // Already set permanently
            if !context.dry_run {
                println!("ZV_DIR: Already set permanently");
            }
            Ok(())
        }
        ZvDirAction::MakePermanent { current_path } => {
            if context.dry_run {
                println!("Would set ZV_DIR={} permanently", current_path.display());
                return Ok(());
            }

            println!("Setting ZV_DIR={} permanently...", current_path.display());

            if context.shell.is_windows_shell() && !context.shell.is_powershell_in_unix() {
                #[cfg(windows)]
                {
                    windows::execute_zv_dir_setup_windows(current_path).await
                }
                #[cfg(not(windows))]
                {
                    Ok(())
                } // This should never be reached
            } else {
                unix::execute_zv_dir_setup_unix(context, current_path).await
            }
        }
    }
}

/// Execute PATH setup based on the determined action
pub async fn execute_path_setup(context: &SetupContext, action: &PathAction) -> crate::Result<()> {
    match action {
        PathAction::AlreadyConfigured => {
            // No action needed
            if !context.dry_run {
                println!("PATH: Already configured with zv bin directory");
            }
            Ok(())
        }
        PathAction::AddToRegistry { bin_path } => {
            if context.dry_run {
                println!(
                    "Would add {} to PATH via Windows registry",
                    bin_path.display()
                );
                return Ok(());
            }

            println!(
                "Adding {} to PATH via Windows registry...",
                bin_path.display()
            );
            #[cfg(windows)]
            {
                windows::execute_path_setup_windows(context, bin_path).await
            }
            #[cfg(not(windows))]
            {
                Ok(())
            } // This should never be reached
        }
        PathAction::GenerateEnvFile {
            env_file_path,
            rc_file,
            bin_path,
        } => {
            if context.dry_run {
                println!(
                    "Would generate env file at {} and modify {}",
                    Paint::blue(&env_file_path.display()),
                    Paint::blue(&rc_file.display())
                );
                return Ok(());
            }

            println!("Generating environment file and updating shell configuration...");
            unix::execute_path_setup_unix(context, env_file_path, rc_file, bin_path).await
        }
    }
}

/// Execute setup phase - coordinate ZV_DIR and PATH actions
pub async fn execute_setup(
    context: &SetupContext,
    requirements: &SetupRequirements,
) -> crate::Result<()> {
    use yansi::Paint;

    if context.dry_run {
        println!("{}", Paint::cyan("🟦 Executing Setup (Dry Run)"));
    } else {
        println!("{}", Paint::green("🟩 Executing Setup"));
    }

    // Execute ZV_DIR setup
    execute_zv_dir_setup(context, &requirements.zv_dir_action)
        .await
        .with_context(|| "ZV_DIR setup failed")?;

    // Execute PATH setup
    execute_path_setup(context, &requirements.path_action)
        .await
        .with_context(|| "PATH setup failed")?;

    // Execute post-setup actions if needed
    if requirements.needs_post_setup {
        post_setup_actions(context)
            .await
            .with_context(|| "Post-setup actions failed")?;
    }

    Ok(())
}

/// Post-setup actions phase - handle binary management and shim regeneration
pub async fn post_setup_actions(context: &SetupContext) -> crate::Result<()> {
    use yansi::Paint;

    if context.dry_run {
        println!("{}", Paint::cyan("→ Post-Setup Actions (Dry Run)"));
        println!("  Would check and update zv binary if needed");
        println!("  Would regenerate shims if binary was updated");
    } else {
        println!("{}", Paint::green("→ Post-Setup Actions"));

        // Use the centralized check_and_update_zv_binary from sync module
        // This will copy the binary AND regenerate shims if needed
        crate::cli::sync::check_and_update_zv_binary(&context.app, true)
            .await
            .with_context(|| "Failed to update zv binary")?;

        // Note: Shim regeneration is now handled inside check_and_update_zv_binary
        // via copy_binary_and_regenerate_shims
    }

    if context.dry_run {
        println!("{}", Paint::cyan("← Post-Setup Actions Complete"));
    } else {
        println!("{}", Paint::green("← Post-Setup Actions Complete"));
    }

    Ok(())
}
