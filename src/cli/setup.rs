use crate::App;
#[cfg(not(target_os = "linux"))]
use crate::shell::setup::{
    InteractiveSetup, SetupContext, apply_user_choices, execute_setup, handle_interactive_error,
    is_recoverable_interactive_error, post_setup_actions, pre_setup_checks,
};
#[cfg(not(target_os = "linux"))]
use color_eyre::eyre::Context as _;
use yansi::Paint;

#[cfg(not(target_os = "linux"))]
/// Print the XDG directory layout table and, if any directories are missing,
/// prompt the user to create them. Returns `false` if the user declined creation.
fn print_dir_table_and_ensure(app: &App) -> crate::Result<bool> {
    use crate::shell::path_utils::check_dir_in_path;
    use std::io::{self, Write};

    let paths = &app.paths;

    // Build table rows: (role, path, status)
    struct Row {
        role: &'static str,
        path: std::path::PathBuf,
    }

    let rows = vec![
        Row { role: "Data  ", path: paths.data_dir.clone() },
        Row { role: "Config", path: paths.config_dir.clone() },
        Row { role: "Cache ", path: paths.cache_dir.clone() },
    ];
    let pub_bin = paths.public_bin_dir.clone();

    // Compute column width for the path column
    let path_width = rows
        .iter()
        .map(|r| r.path.display().to_string().len())
        .chain(pub_bin.iter().map(|p| p.display().to_string().len()))
        .max()
        .unwrap_or(30)
        .max(30);

    let sep = "─".repeat(8 + path_width + 14);
    println!();
    println!("{}", Paint::cyan("zv directory layout (XDG Base Directory Specification)").bold());
    println!("{sep}");
    println!("  {:<8}  {:<path_width$}  Status", "Role", "Directory");
    println!("{sep}");

    let mut dirs_to_create: Vec<std::path::PathBuf> = Vec::new();

    for row in &rows {
        let status = if row.path.is_dir() {
            Paint::green("✓ exists").to_string()
        } else {
            dirs_to_create.push(row.path.clone());
            Paint::yellow("[will create]").to_string()
        };
        println!(
            "  {:<8}  {:<path_width$}  {}",
            row.role,
            row.path.display(),
            status
        );
    }

    // Public bin row (XDG only)
    if let Some(ref pub_bin_path) = pub_bin {
        let in_path = check_dir_in_path(pub_bin_path);
        let status = if !pub_bin_path.is_dir() {
            dirs_to_create.push(pub_bin_path.clone());
            Paint::yellow("[will create]").to_string()
        } else if in_path {
            Paint::green("✓ in PATH").to_string()
        } else {
            Paint::yellow("exists, not in PATH").to_string()
        };
        println!(
            "  {:<8}  {:<path_width$}  {}",
            "Pub bin",
            pub_bin_path.display(),
            status
        );
    }

    println!("{sep}");
    println!();

    // Prompt for directory creation if needed
    if !dirs_to_create.is_empty() {
        println!("{}", Paint::yellow("Directories to create:"));
        for dir in &dirs_to_create {
            println!("  • {}", Paint::cyan(&dir.display().to_string()));
        }
        println!();

        if !crate::tools::supports_interactive_prompts() {
            // Non-interactive: create without asking
            for dir in &dirs_to_create {
                std::fs::create_dir_all(dir)?;
            }
        } else {
            print!("Create these directories? [Y/n] ");
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            let trimmed = input.trim().to_lowercase();
            if trimmed == "n" || trimmed == "no" {
                println!("{}", Paint::red("Aborted."));
                return Ok(false);
            }
            for dir in &dirs_to_create {
                std::fs::create_dir_all(dir)?;
                println!("  {} Created {}", Paint::green("✓"), dir.display());
            }
        }
        println!();
    }

    Ok(true)
}

/// Main setup_shell function that orchestrates the three-phase setup process
/// This is the public interface that maintains backward compatibility and supports interactive mode

pub async fn setup_shell(
    #[allow(unused_variables)] app: &mut App,
    #[allow(unused_variables)] using_env_var: bool,
    #[allow(unused_variables)] dry_run: bool,
    #[allow(unused_variables)] no_interactive: bool,
) -> crate::Result<()> {
    // On Linux, zv setup is a no-op — XDG dirs handle everything
    #[cfg(target_os = "linux")]
    {
        println!(
            "{} No setup needed. Your system uses XDG directories. Run {} to initialize.",
            Paint::green("✓"),
            Paint::blue("zv sync")
        );
        return Ok(());
    }

    // On macOS Tier 1 (XDG dirs exist), same as Linux
    #[cfg(target_os = "macos")]
    if app.paths.tier == 1 && !using_env_var {
        println!(
            "{} No setup needed. Your system uses XDG directories. Run {} to initialize.",
            Paint::green("✓"),
            Paint::blue("zv sync")
        );
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
    if !dry_run {
        let proceed = print_dir_table_and_ensure(app)?;
        if !proceed {
            return Ok(());
        }
    }

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
}

#[cfg(not(target_os = "linux"))]
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
    if let Ok(term) = std::env::var("TERM")
        && term == "dumb"
    {
        return false;
    }

    // Check if TTY is available for interactive prompts
    crate::tools::supports_interactive_prompts()
}
