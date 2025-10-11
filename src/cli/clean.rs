use crate::app::toolchain::ToolchainManager;
use crate::cli::CleanTarget;
use crate::{App, ResolvedZigVersion, ZigVersion};
use yansi::Paint;

pub async fn clean(
    app: &mut App,
    target: Option<CleanTarget>,
    except: Vec<ZigVersion>,
    outdated: bool,
) -> crate::Result<()> {
    // Handle --outdated flag
    // If --outdated is specified without a target or with 'master', clean outdated master versions
    if outdated {
        let should_clean_outdated = match &target {
            None => true, // `zv clean --outdated`
            Some(CleanTarget::Versions(versions)) => {
                // `zv clean master --outdated` or similar
                versions
                    .iter()
                    .any(|ver| matches!(ver, ZigVersion::Master(_)))
            }
            _ => false,
        };

        if should_clean_outdated {
            return clean_outdated_master(app).await;
        } else {
            eprintln!(
                "{} --outdated flag can only be used where clean target is 'master'",
                Paint::red("✗")
            );
            eprintln!(
                "{} Usage: zv clean --outdated  OR  zv clean master --outdated",
                Paint::yellow("ℹ")
            );
            return Ok(());
        }
    }

    // Handle --except flag
    if !except.is_empty() {
        return clean_except_versions(app, except).await;
    }

    // Handle target-based cleaning
    match target {
        None => clean_all(app).await,
        Some(CleanTarget::All) => clean_all(app).await,
        Some(CleanTarget::Downloads) => clean_downloads_only(app).await,
        Some(CleanTarget::Versions(versions)) => clean_specific_versions(app, versions).await,
        Some(CleanTarget::Zls) => {
            println!(
                "{} todo: route to zv zls clean | rm all --except <active zig version>",
                Paint::yellow("⚠")
            );
            Ok(())
        }
    }
}

/// Clean specific versions from the comma-separated list
async fn clean_specific_versions(app: &mut App, versions: Vec<ZigVersion>) -> crate::Result<()> {
    // Deduplicate semver variants
    let versions = crate::tools::deduplicate_semver_variants(versions);

    // Format the version list for display
    let version_list: Vec<String> = versions
        .iter()
        .map(|v| match v {
            ZigVersion::Semver(ver) => ver.to_string(),
            ZigVersion::Master(Some(ver)) => format!("master/{}", ver),
            ZigVersion::Master(None) => "master".to_string(),
            _ => format!("{:?}", v),
        })
        .collect();

    let versions_display = if version_list.len() == 1 {
        version_list[0].clone()
    } else {
        version_list.join(", ")
    };

    println!(
        "{}",
        Paint::cyan(&format!("Removing version(s): {}", versions_display)).bold()
    );

    // Get all installed versions with their paths using scan_installations
    let installations = ToolchainManager::scan_installations(&app.versions_path)?;
    let active_install = app.toolchain_manager.get_active_install();

    let mut removed_count = 0;
    let mut not_found_count = 0;
    let mut failed_count = 0;
    let mut active_version_removed = false;

    for version in versions {
        // Find if this version is actually installed
        let installation = installations.iter().find(|install| {
            match &version {
                ZigVersion::Semver(target_v) => !install.is_master && &install.version == target_v,
                ZigVersion::Master(Some(target_v)) => {
                    install.is_master && &install.version == target_v
                }
                ZigVersion::Master(None) => install.is_master, // Match any master version
                ZigVersion::Stable(Some(target_v)) | ZigVersion::Latest(Some(target_v)) => {
                    // Stable@version and Latest@version should match like regular semver
                    !install.is_master && &install.version == target_v
                }
                ZigVersion::Stable(None) | ZigVersion::Latest(None) => {
                    // Match the highest stable version (non-master)
                    !install.is_master && {
                        // Find the highest stable version among all installations
                        let highest_stable = installations
                            .iter()
                            .filter(|i| !i.is_master)
                            .max_by(|a, b| a.version.cmp(&b.version));

                        if let Some(highest) = highest_stable {
                            &install.version == &highest.version
                        } else {
                            false
                        }
                    }
                }
            }
        });

        match installation {
            Some(install) => {
                // Check if we're removing the currently active version
                let is_active = active_install.is_some_and(|active| {
                    active.version == install.version && active.is_master == install.is_master
                });

                if is_active {
                    active_version_removed = true;
                    let display_name = if install.is_master {
                        format!("master/{}", install.version)
                    } else {
                        install.version.to_string()
                    };
                    println!(
                        "{} Warning: Removing currently active version: {}",
                        Paint::yellow("⚠"),
                        display_name
                    );
                }

                match tokio::fs::remove_dir_all(&install.path).await {
                    Ok(()) => {
                        removed_count += 1;
                        let display_name = if install.is_master {
                            format!("master/{}", install.version)
                        } else {
                            install.version.to_string()
                        };
                        println!("{} Removed: {}", Paint::red("✗"), display_name);
                    }
                    Err(e) => {
                        failed_count += 1;
                        let display_name = if install.is_master {
                            format!("master/{}", install.version)
                        } else {
                            install.version.to_string()
                        };
                        eprintln!(
                            "{} Failed to remove {}: {}",
                            Paint::red("✗"),
                            display_name,
                            e
                        );
                    }
                }
            }
            None => {
                not_found_count += 1;
                let display_name = match version {
                    ZigVersion::Semver(v) => v.to_string(),
                    ZigVersion::Master(Some(v)) => format!("master/{}", v),
                    ZigVersion::Master(None) => "master".to_string(),
                    _ => format!("{:?}", version),
                };
                println!("{} Version {} not found", Paint::yellow("⚠"), display_name);
            }
        }
    }

    // Handle active version removal by automatically selecting a new active version
    if active_version_removed {
        handle_active_version_removal(app).await?;
    }

    // Provide summary feedback
    let mut summary_parts = Vec::new();
    if removed_count > 0 {
        summary_parts.push(format!("{} removed", removed_count));
    }
    if not_found_count > 0 {
        summary_parts.push(format!("{} not found", not_found_count));
    }
    if failed_count > 0 {
        summary_parts.push(format!("{} failed", failed_count));
    }

    let summary = if summary_parts.is_empty() {
        "No versions processed".to_string()
    } else {
        summary_parts.join(", ")
    };

    let icon = if failed_count > 0 {
        Paint::yellow("⚠")
    } else {
        Paint::green("✓")
    };

    println!("{} Cleanup completed: {}", icon, summary);

    Ok(())
}

/// Clean all versions except the specified ones
async fn clean_except_versions(
    app: &mut App,
    except_versions: Vec<ZigVersion>,
) -> crate::Result<()> {
    // Deduplicate semver variants
    let except_versions = crate::tools::deduplicate_semver_variants(except_versions);

    // Format the except version list for display
    let except_list: Vec<String> = except_versions
        .iter()
        .map(|v| match v {
            ZigVersion::Semver(ver) => ver.to_string(),
            ZigVersion::Master(Some(ver)) => format!("master/{}", ver),
            ZigVersion::Master(None) => "master".to_string(),
            _ => format!("{:?}", v),
        })
        .collect();

    let except_display = if except_list.len() == 1 {
        except_list[0].clone()
    } else {
        except_list.join(", ")
    };

    println!(
        "{}",
        Paint::cyan(&format!("Removing all versions except: {}", except_display)).bold()
    );

    // Get all installed versions using scan_installations
    let installations = ToolchainManager::scan_installations(&app.versions_path)?;
    let active_install = app.toolchain_manager.get_active_install();
    let mut removed_count = 0;
    let mut kept_count = 0;
    let mut failed_count = 0;
    let mut active_version_removed = false;

    // Track which except versions were actually found
    let mut found_except_versions = std::collections::HashSet::new();

    for install in &installations {
        let should_keep = except_versions.iter().any(|except_ver| {
            let matches = match except_ver {
                ZigVersion::Semver(v) => !install.is_master && v == &install.version,
                ZigVersion::Master(Some(v)) => install.is_master && v == &install.version,
                ZigVersion::Master(None) => install.is_master,
                _ => false,
            };

            if matches {
                found_except_versions.insert(except_ver.clone());
            }

            matches
        });

        if should_keep {
            kept_count += 1;
            let display_name = if install.is_master {
                format!("master/{}", install.version)
            } else {
                install.version.to_string()
            };
            println!("{} Kept: {}", Paint::green("✓"), display_name);
        } else {
            // Check if we're removing the currently active version
            let is_active = active_install.is_some_and(|active| active == install);

            if is_active {
                active_version_removed = true;
                let display_name = if install.is_master {
                    format!("master/{}", install.version)
                } else {
                    install.version.to_string()
                };
                println!(
                    "{} Warning: Removing currently active version: {}",
                    Paint::yellow("⚠"),
                    display_name
                );
            }

            match tokio::fs::remove_dir_all(&install.path).await {
                Ok(()) => {
                    removed_count += 1;
                    let display_name = if install.is_master {
                        format!("master/{}", install.version)
                    } else {
                        install.version.to_string()
                    };
                    println!("{} Removed: {}", Paint::red("✗"), display_name);
                }
                Err(e) => {
                    failed_count += 1;
                    let display_name = if install.is_master {
                        format!("master/{}", install.version)
                    } else {
                        install.version.to_string()
                    };
                    eprintln!(
                        "{} Failed to remove {}: {}",
                        Paint::red("✗"),
                        display_name,
                        e
                    );
                }
            }
        }
    }

    // Report non-existent versions in --except list (Requirement 4.3)
    for except_ver in &except_versions {
        if !found_except_versions.contains(except_ver) {
            let display_name = match except_ver {
                ZigVersion::Semver(v) => v.to_string(),
                ZigVersion::Master(Some(v)) => format!("master/{}", v),
                ZigVersion::Master(None) => "master".to_string(),
                _ => format!("{:?}", except_ver),
            };
            println!(
                "{} Version {} not found (specified in --except)",
                Paint::yellow("⚠"),
                display_name
            );
        }
    }

    // Report when no cleanup was needed (Requirement 4.4)
    if removed_count == 0 && failed_count == 0 {
        println!(
            "{} No cleanup needed - all installed versions were in the --except list",
            Paint::green("✓")
        );
    } else {
        // Provide summary feedback
        let mut summary_parts = Vec::new();
        if removed_count > 0 {
            summary_parts.push(format!("{} removed", removed_count));
        }
        if kept_count > 0 {
            summary_parts.push(format!("{} kept", kept_count));
        }
        if failed_count > 0 {
            summary_parts.push(format!("{} failed", failed_count));
        }

        let summary = summary_parts.join(", ");
        let icon = if failed_count > 0 {
            Paint::yellow("⚠")
        } else {
            Paint::green("✓")
        };

        println!("{} Cleanup completed: {}", icon, summary);
    }

    // Handle active version removal by automatically selecting a new active version
    if active_version_removed {
        handle_active_version_removal(app).await?;
    }

    Ok(())
}

/// Clean outdated master versions, keeping only the latest
async fn clean_outdated_master(app: &mut App) -> crate::Result<()> {
    println!(
        "{}",
        Paint::cyan("Removing outdated master versions...").bold()
    );

    let master_path = app.versions_path.join("master");
    if !master_path.exists() {
        println!("{} No master directory found", Paint::yellow("⚠"));
        return Ok(());
    }

    // Get all master installations using scan_installations
    let installations = ToolchainManager::scan_installations(&app.versions_path)?;
    let active_install = app.toolchain_manager.get_active_install();
    let mut master_installs: Vec<_> = installations
        .into_iter()
        .filter(|install| install.is_master)
        .collect();

    if master_installs.is_empty() {
        println!("{} No master versions found", Paint::yellow("⚠"));
        return Ok(());
    }

    // Sort to find the latest (highest version)
    master_installs.sort_by(|a, b| a.version.cmp(&b.version));
    let latest_master = master_installs.last().unwrap();

    let mut removed_count = 0;
    let mut active_version_removed = false;

    // Remove all master versions except the latest
    for install in &master_installs {
        if install.version != latest_master.version {
            // Check if we're removing the currently active version
            let is_active = active_install.is_some_and(|active| active == install);

            if is_active {
                active_version_removed = true;
                println!(
                    "{} Warning: Removing currently active version: master/{}",
                    Paint::yellow("⚠"),
                    install.version
                );
            }

            match tokio::fs::remove_dir_all(&install.path).await {
                Ok(()) => {
                    removed_count += 1;
                    println!(
                        "{} Removed outdated: master/{}",
                        Paint::red("✗"),
                        install.version
                    );
                }
                Err(e) => {
                    eprintln!(
                        "{} Failed to remove master/{}: {}",
                        Paint::red("✗"),
                        install.version,
                        e
                    );
                }
            }
        }
    }

    if removed_count == 0 {
        println!(
            "{} No outdated master versions to remove",
            Paint::green("✓")
        );
    } else {
        println!(
            "{} Removed {} outdated master version(s), kept latest: master/{}",
            Paint::green("✓"),
            removed_count,
            latest_master.version
        );
    }

    // Handle active version removal by automatically selecting a new active version
    if active_version_removed {
        handle_active_version_removal(app).await?;
    }

    Ok(())
}

/// Clean downloads directory only
async fn clean_downloads_only(app: &mut App) -> crate::Result<()> {
    let downloads_path = app.download_cache();
    println!("{}", Paint::cyan("Cleaning downloads directory...").bold());

    // Check if downloads directory exists
    if !downloads_path.exists() {
        println!(
            "{} Downloads directory doesn't exist: {}",
            Paint::yellow("⚠"),
            downloads_path.display()
        );
        return Ok(());
    }

    // Remove the entire downloads directory
    match tokio::fs::remove_dir_all(&downloads_path).await {
        Ok(()) => {
            println!("{} Removed downloads directory", Paint::red("✗"));
        }
        Err(e) => {
            eprintln!(
                "{} Failed to remove downloads directory: {}",
                Paint::red("✗"),
                e
            );
            return Err(color_eyre::eyre::eyre!(
                "Failed to remove downloads directory: {}",
                e
            ));
        }
    }

    // Recreate downloads directory with tmp subfolder
    match tokio::fs::create_dir_all(&downloads_path.join("tmp")).await {
        Ok(()) => {
            println!(
                "{} Successfully cleaned downloads directory",
                Paint::green("✓")
            );
        }
        Err(e) => {
            eprintln!(
                "{} Failed to recreate downloads/tmp directory: {}",
                Paint::yellow("⚠"),
                e
            );
            return Err(color_eyre::eyre::eyre!(
                "Failed to recreate downloads directory: {}",
                e
            ));
        }
    }

    Ok(())
}

/// Clean up all contents of the versions directory (for clean_all command)
pub async fn clean_all_versions(app: &mut App) -> crate::Result<()> {
    let versions_path = &app.versions_path;

    println!("{}", Paint::cyan("Removing all versions...").bold());

    if !versions_path.exists() {
        println!(
            "{} Versions directory doesn't exist: {}",
            Paint::yellow("⚠"),
            versions_path.display()
        );
        return Ok(());
    }

    // Remove the entire versions directory
    match tokio::fs::remove_dir_all(versions_path).await {
        Ok(()) => {
            println!("{} Removed versions directory", Paint::red("✗"));
        }
        Err(e) => {
            eprintln!(
                "{} Failed to remove versions directory: {}",
                Paint::red("✗"),
                e
            );
            return Err(color_eyre::eyre::eyre!(
                "Failed to remove versions directory: {}",
                e
            ));
        }
    }

    // Recreate the versions directory
    match tokio::fs::create_dir(versions_path).await {
        Ok(()) => {
            println!(
                "{} Successfully cleaned versions directory",
                Paint::green("✓")
            );
        }
        Err(e) => {
            eprintln!(
                "{} Failed to recreate versions directory: {}",
                Paint::yellow("⚠"),
                e
            );
            return Err(color_eyre::eyre::eyre!(
                "Failed to recreate versions directory: {}",
                e
            ));
        }
    }

    Ok(())
}

pub async fn clean_downloads(app: &mut App) -> crate::Result<()> {
    let downloads_path = app.download_cache();
    println!("{}", Paint::cyan("Cleaning downloads directory...").bold());

    if !downloads_path.exists() {
        println!("{} Downloads directory doesn't exist", Paint::yellow("⚠"));
        return Ok(());
    }

    // Remove the entire downloads directory
    match tokio::fs::remove_dir_all(&downloads_path).await {
        Ok(()) => {
            println!("{} Removed downloads directory", Paint::red("✗"));
        }
        Err(e) => {
            eprintln!(
                "{} Failed to remove downloads directory: {}",
                Paint::red("✗"),
                e
            );
            return Err(color_eyre::eyre::eyre!(
                "Failed to remove downloads directory: {}",
                e
            ));
        }
    }

    // Recreate downloads directory with tmp subfolder
    match tokio::fs::create_dir_all(&downloads_path.join("tmp")).await {
        Ok(()) => {
            println!(
                "{} Successfully cleaned downloads directory",
                Paint::green("✓")
            );
        }
        Err(e) => {
            eprintln!(
                "{} Failed to recreate downloads/tmp directory: {}",
                Paint::yellow("⚠"),
                e
            );
            return Err(color_eyre::eyre::eyre!(
                "Failed to recreate downloads directory: {}",
                e
            ));
        }
    }

    Ok(())
}

/// Clean up both bin and versions directories
pub async fn clean_all(app: &mut App) -> crate::Result<()> {
    println!("{}", Paint::cyan("Performing full cleanup...").bold());

    // Note: bin directory cleanup has been removed - shims are managed by 'zv use <version>'

    // Clean all contents of versions/ directory (enhanced functionality)
    clean_all_versions(app).await?;
    println!(); // Add spacing

    // Clean downloads/ directory and recreate with tmp subfolder
    clean_downloads(app).await?;
    println!();

    println!("{}", Paint::green("Full cleanup completed!").bold());
    Ok(())
}

/// Handle active version removal by automatically selecting a new active version
/// Priority: highest stable > highest master > none
async fn handle_active_version_removal(app: &mut App) -> crate::Result<()> {
    println!();

    // Get all remaining installed versions
    let installations = ToolchainManager::scan_installations(&app.versions_path)?;

    if installations.is_empty() {
        println!(
            "{} No Zig versions remain installed. Run 'zv use <version>' to install and activate a version.",
            Paint::cyan("ℹ")
        );
        return Ok(());
    }

    // Find the best replacement version using priority: highest stable > highest master > none
    let new_active = installations
        .iter()
        .filter(|install| !install.is_master)
        .max_by(|a, b| a.version.cmp(&b.version))
        .map(|install| (install, false))
        .or_else(|| {
            installations
                .iter()
                .filter(|install| install.is_master)
                .max_by(|a, b| a.version.cmp(&b.version))
                .map(|install| (install, true))
        });

    match new_active {
        Some((install, is_master)) => {
            if is_master {
                println!(
                    "{} Automatically setting new active version: master <{}>",
                    Paint::cyan("→"),
                    Paint::yellow(&install.version)
                );
            } else {
                println!(
                    "{} Automatically setting new active version: <{}>",
                    Paint::cyan("→"),
                    Paint::yellow(&install.version)
                );
            };

            // Create ResolvedZigVersion for the new active version
            let resolved_version = if is_master {
                ResolvedZigVersion::Master(install.version.clone())
            } else {
                ResolvedZigVersion::Semver(install.version.clone())
            };

            // Use app's set_active_version method with the installation path to skip scanning
            match app
                .set_active_version(&resolved_version, Some(install.path.clone()))
                .await
            {
                Ok(()) => {
                    println!(
                        "{} Successfully set active version to: {}",
                        Paint::green("✓"),
                        Paint::yellow(&install.version),
                    );
                }
                Err(e) => {
                    eprintln!(
                        "{} Failed to set active version to {}: {e}",
                        Paint::red("✗"),
                        Paint::yellow(&install.version),
                    );
                    println!(
                        "{} Run 'zv use {}' to manually set the active version.",
                        Paint::cyan("ℹ"),
                        Paint::yellow(&install.version),
                    );
                }
            }
        }
        None => {
            println!(
                "{} No Zig versions remain installed. Run 'zv use <version>' to install and activate a version.",
                Paint::cyan("ℹ")
            );
            let _ = app.toolchain_manager.clear_active_version();
        }
    }

    Ok(())
}
