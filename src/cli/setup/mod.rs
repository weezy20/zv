use cfg_if::cfg_if;
use color_eyre::eyre::{Context as _, eyre};
use std::fs::File;
use std::io::Read;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use yansi::Paint;
mod setup_utils;
use crate::tools::{calculate_file_hash, canonicalize, files_have_same_hash};
use crate::{App, Shell, ZigVersion, suggest, tools};
use setup_utils::*;

#[cfg(target_os = "windows")]
pub mod windows;
#[cfg(target_os = "windows")]
pub use windows::setup_windows_environment;

pub mod unix;
pub use unix::{add_source_to_file, add_source_to_shell_files, setup_unix_environment};

/// Perform post-setup actions: copy zv binary and regenerate shims
async fn post_setup_actions(app: &App, dry_run: bool) -> crate::Result<()> {
    if dry_run {
        println!(
            "\n{} post-setup actions:",
            Paint::yellow("Dry run: would perform")
        );
    } else {
        println!("\nPerforming post-setup actions:");
    }

    // Copy zv binary to bin directory if needed
    // Replaces previous bin if current-exe is newer or if same versions, but different hashes.
    copy_zv_binary_if_needed(app, dry_run).await?;

    // Regenerate shims if needed
    regenerate_shims_if_needed(app, dry_run).await?;

    Ok(())
}

pub async fn setup_shell(
    app: &mut App,
    using_env_var: bool,
    dry_run: bool,
    default_version: Option<ZigVersion>,
) -> crate::Result<()> {
    if app.source_set {
        println!(
            "{}",
            Paint::green("Shell environment PATH is already set up for zv binaries.")
        );

        // Even when shell environment is set up, we need to check if binary needs updating
        // or if shims need regeneration
        post_setup_actions(app, dry_run).await?;
        return Ok(());
    }
    // App::init() for zv_main() ensures shell is always here
    // but in the rare case, fallback to default which calls Shell::detect()
    let shell = app.shell.unwrap_or_default();

    // Perform pre-setup checks to see if setup is actually needed
    if !dry_run {
        let setup_requirements = pre_setup_checks(app, &shell, using_env_var).await?;
        match setup_requirements {
            None => {
                // No setup needed, but still check post-setup actions
                post_setup_actions(app, dry_run).await?;
                return Ok(());
            }
            Some(requirements) => {
                // Setup is needed, proceed with the requirements
                if dry_run {
                    println!(
                        "{} zv setup for {} shell...",
                        Paint::yellow("Previewing"),
                        Paint::cyan(&shell.to_string())
                    );
                } else {
                    println!(
                        "Setting up zv for {} shell...",
                        Paint::cyan(&shell.to_string())
                    );
                }

                cfg_if! {
                    if #[cfg(target_os = "windows")] {
                        setup_windows_environment(app, &requirements, dry_run).await?;
                    } else {
                        setup_unix_environment(app, &shell, &requirements, dry_run).await?;
                    }
                }

                // Perform post-setup actions if required
                if requirements.perform_post_setup_action {
                    post_setup_actions(app, dry_run).await?;
                }
            }
        }
    } else {
        // For dry run, show what would be done
        println!(
            "{} zv setup for {} shell...",
            Paint::yellow("Previewing"),
            Paint::cyan(&shell.to_string())
        );

        cfg_if! {
            if #[cfg(target_os = "windows")] {
                let dummy_requirements = setup_utils::SetupRequirements {
                    set_zv_dir_env: using_env_var,
                    generate_env_file: false,
                    edit_rc_file: false,
                    perform_post_setup_action: true,
                };
                setup_windows_environment(app, &dummy_requirements, dry_run).await?;
            } else {
                let dummy_requirements = setup_utils::SetupRequirements {
                    set_zv_dir_env: using_env_var,
                    generate_env_file: true,
                    edit_rc_file: true,
                    perform_post_setup_action: true,
                };
                setup_unix_environment(app, &shell, &dummy_requirements, dry_run).await?;
            }
        }

        // For dry run, always show what post-setup actions would be done
        post_setup_actions(app, dry_run).await?;
    }

    Ok(())
}
