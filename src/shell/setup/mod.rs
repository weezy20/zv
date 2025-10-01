use crate::{app::App, tools::files_have_same_hash};
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
    if let Ok(term) = std::env::var("TERM") {
        if term == "dumb" {
            return false;
        }
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
    } else {
        println!("{}", Paint::green("→ Post-Setup Actions"));
    }

    // Copy zv binary to bin directory if needed
    copy_zv_binary_if_needed(&context.app, context.dry_run)
        .await
        .with_context(|| "Failed to copy zv binary")?;

    // Regenerate shims if needed
    regenerate_shims_if_needed(&context.app, context.dry_run)
        .await
        .with_context(|| "Failed to regenerate shims")?;

    if context.dry_run {
        println!("{}", Paint::cyan("← Post-Setup Actions Complete"));
    } else {
        println!("{}", Paint::green("← Post-Setup Actions Complete"));
    }

    Ok(())
}

/// Copy the current zv binary to the bin directory if needed
pub async fn copy_zv_binary_if_needed(app: &App, dry_run: bool) -> crate::Result<()> {
    use yansi::Paint;

    let current_exe = std::env::current_exe().map_err(|e| {
        crate::ZvError::shell_post_setup_action_failed(&format!(
            "Failed to get current executable path: {}",
            e
        ))
    })?;

    let target_exe = if cfg!(windows) {
        app.bin_path().join("zv.exe")
    } else {
        app.bin_path().join("zv")
    };

    // Check if target exists and compare hashes
    if target_exe.exists() {
        match files_have_same_hash(&current_exe, &target_exe) {
            Ok(true) => {
                if !dry_run {
                    println!("✓ zv binary present ({})", Paint::green(&target_exe.display()));
                }
                return Ok(());
            }
            Ok(false) => {
                if dry_run {
                    println!("Would update zv binary in bin directory (checksum mismatch)");
                } else {
                    println!("Updating zv binary in bin directory (checksum mismatch)...");
                }
            }
            Err(e) => {
                if !dry_run {
                    println!(
                        "⚠ Warning: checksum comparison failed: {}, will copy anyway",
                        e
                    );
                }
            }
        }
    } else {
        if dry_run {
            println!("Would copy zv binary to bin directory");
        } else {
            println!("Copying zv binary to bin directory...");
        }
    }

    if !dry_run {
        // Create bin directory if it doesn't exist
        tokio::fs::create_dir_all(app.bin_path())
            .await
            .map_err(|e| {
                crate::ZvError::shell_post_setup_action_failed(&format!(
                    "Failed to create bin directory: {}",
                    e
                ))
            })?;

        // Copy the current executable to the target location
        tokio::fs::copy(&current_exe, &target_exe)
            .await
            .map_err(|e| {
                crate::ZvError::shell_post_setup_action_failed(&format!(
                    "Failed to copy zv binary to bin directory: {}",
                    e
                ))
            })?;

        println!(
            "✓ Copied {} to {}",
            Paint::green(&current_exe.display().to_string()),
            Paint::green(&target_exe.display().to_string())
        );
    }

    Ok(())
}

/// Regenerate hardlinks/shims for zig and zls if they exist and active version is available
pub async fn regenerate_shims_if_needed(app: &App, dry_run: bool) -> crate::Result<()> {
    use crate::app::toolchain::ToolchainManager;
    use crate::types::Shim;
    
    let zig_shim = app.bin_path().join(Shim::Zig.executable_name());
    let zls_shim = app.bin_path().join(Shim::Zls.executable_name());

    let has_zig_shim = zig_shim.exists();
    let has_zls_shim = zls_shim.exists();

    if !has_zig_shim && !has_zls_shim {
        if !dry_run {
            println!("No pre-existing zig/zls shims found - nothing to regenerate");
        }
        return Ok(());
    }

    // Check if active.json exists (contains serialized ZigInstall)
    let active_path = app.path().join("active.json");
    if !active_path.exists() {
        if has_zig_shim || has_zls_shim {
            if dry_run {
                println!("Would skip shim regeneration - no active version configured");
            } else {
                println!("⚠ No active version configured - cannot regenerate shims");
                println!("  Run 'zv use <version>' to set up configuration");
            }
        }
        return Ok(());
    }

    if dry_run {
        if has_zig_shim {
            println!("Would regenerate zig shim based on active version");
        }
        if has_zls_shim {
            println!("Would regenerate zls shim based on active version");
        }
    } else {
        // Actually regenerate shims using the toolchain manager
        if has_zig_shim || has_zls_shim {
            println!("Regenerating shims based on active version...");
            
            match ToolchainManager::new(app.path()).await {
                Ok(mut toolchain) => {
                    if let Some(active_install) = toolchain.get_active_install() {
                        // Use the existing deploy_shims method from toolchain manager
                        if let Err(e) = toolchain.deploy_shims(active_install).await {
                            println!("⚠ Failed to regenerate shims: {}", e);
                            println!("  Run 'zv use <version>' to ensure shims are properly configured");
                        } else {
                            println!("✓ Successfully regenerated shims for version {}", active_install.version);
                        }
                    } else {
                        println!("⚠ No active installation found");
                        println!("  Run 'zv use <version>' to set up configuration");
                    }
                }
                Err(e) => {
                    println!("⚠ Failed to initialize toolchain manager: {}", e);
                    println!("  Run 'zv use <version>' to ensure shims are properly configured");
                }
            }
        }
    }

    Ok(())
}
