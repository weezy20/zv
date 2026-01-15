use crate::app::toolchain::ToolchainManager;
use crate::cli::CleanTarget;
use crate::{App, ResolvedZigVersion, ZigVersion};
use yansi::Paint;

pub async fn clean(
    app: &mut App,
    targets: Vec<CleanTarget>,
    except: Vec<ZigVersion>,
    outdated: bool,
) -> crate::Result<()> {
    // Handle --outdated flag
    if outdated {
        let should_clean_outdated = if targets.is_empty() {
            true
        } else {
            targets.iter().any(|t| matches!(t, CleanTarget::Versions(versions) if versions.iter().any(|v| matches!(v, ZigVersion::Master(_)))))
        };

        if should_clean_outdated {
            return clean_outdated_master(app).await;
        } else {
            return Ok(());
        }
    }

    // Handle --except flag
    if !except.is_empty() {
        return clean_except_versions(app, except).await;
    }

    // Strict Target Parsing
    let mut should_clean_all = false;
    let mut should_clean_downloads = false;

    let has_all = targets.iter().any(|t| matches!(t, CleanTarget::All));
    let has_versions = targets
        .iter()
        .any(|t| matches!(t, CleanTarget::Versions(_)));

    // Validate mutual exclusivity
    if has_all && has_versions {
        eprintln!(
            "{} Usage: zv clean [all] OR zv clean <version>...",
            Paint::red("✗")
        );
        return Ok(());
    }

    let mut specific_versions = Vec::new();

    if targets.is_empty() {
        // No targets -> prompt for all
        if !confirm_clean_all()? {
            return Ok(());
        }
        should_clean_all = true;
        should_clean_downloads = true;
    } else if has_all {
        should_clean_all = true;
        should_clean_downloads = true;
    } else {
        // Collect versions
        for target in targets {
            match target {
                CleanTarget::Versions(versions) => specific_versions.extend(versions),
                CleanTarget::Downloads => should_clean_downloads = true,
                _ => {}
            }
        }
    }

    if should_clean_all {
        clean_all_versions(app).await?;
    } else if !specific_versions.is_empty() {
        clean_specific_versions(app, specific_versions).await?;
    }

    if should_clean_downloads {
        clean_downloads(app).await?;
    }

    // Summary
    if should_clean_all && should_clean_downloads {
        println!("{}", Paint::green("Full cleanup completed!").bold());
    }

    Ok(())
}

fn confirm_clean_all() -> crate::Result<bool> {
    if !crate::tools::supports_interactive_prompts() {
        return Ok(true); // Assume yes in non-interactive mode
    }

    use dialoguer::theme::ColorfulTheme;

    println!();
    println!(
        "{}",
        Paint::yellow("WARNING: This will remove ALL installed Zig versions and cached downloads.")
            .bold()
    );

    dialoguer::Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Are you sure you want to continue?")
        .default(true)
        .interact()
        .map_err(|e| crate::ZvError::from(color_eyre::eyre::eyre!(e)).into())
}

/// Clean specific versions from the list
async fn clean_specific_versions(app: &mut App, versions: Vec<ZigVersion>) -> crate::Result<()> {
    // Get local master version early for resolution
    let local_master_version: Option<String> = app.toolchain_manager.get_local_master_version();

    // Resolve Master(None) to Master(Some(v)) if local_master_version is available
    let versions: Vec<ZigVersion> = versions
        .into_iter()
        .map(|v| match v {
            ZigVersion::Master(None) => {
                if let Some(ref master_ver_str) = local_master_version {
                    // Parse the local master version string to a Version
                    if let Ok(master_ver) = semver::Version::parse(master_ver_str) {
                        ZigVersion::Master(Some(master_ver))
                    } else {
                        v // Keep as Master(None) if parsing fails
                    }
                } else {
                    v // Keep as Master(None) if no local master
                }
            }
            _ => v,
        })
        .collect();

    // Deduplicate semver variants
    let versions = crate::tools::deduplicate_semver_variants(versions);

    // Format the version list for display
    let version_list: Vec<String> = versions.iter().map(|v| v.to_string()).collect();

    let versions_display = if version_list.len() == 1 {
        version_list[0].clone()
    } else {
        version_list.join(", ")
    };

    println!(
        "{}",
        Paint::cyan(&format!("Removing version(s): {}", versions_display)).bold()
    );

    let installations = ToolchainManager::scan_installations(&app.versions_path)?;
    let active_install = app.toolchain_manager.get_active_install().cloned();

    let mut removed_count = 0;
    let mut not_found_count = 0;
    let mut failed_count = 0;
    let mut active_version_removed = false;
    let mut master_version_removed = false;

    for version in versions {
        let installation = match version {
            ZigVersion::Master(Some(ref v)) => {
                // Target specific master version - just match on the semver
                installations.iter().find(|i| &i.version == v)
            }
            ZigVersion::Master(None) => {
                // Target generic master - prefer local_master_version if it exists
                if let Some(ref local_master) = local_master_version {
                    installations
                        .iter()
                        .find(|i| i.is_master && i.version.to_string() == *local_master)
                } else {
                    // fallback to any master? or maybe latest master?
                    // current logic was: find ANY master (first one found)
                    installations.iter().find(|i| i.is_master)
                }
            }
            _ => installations.iter().find(|install| match &version {
                ZigVersion::Semver(target_v) => !install.is_master && &install.version == target_v,
                ZigVersion::Stable(Some(target_v)) | ZigVersion::Latest(Some(target_v)) => {
                    !install.is_master && &install.version == target_v
                }
                _ => false,
            }),
        };

        match installation {
            Some(install) => {
                let is_active = active_install.as_ref().is_some_and(|active| {
                    active.version == install.version && active.is_master == install.is_master
                });

                if is_active {
                    active_version_removed = true;
                    println!(
                        "{} Warning: Removing currently active version: {}",
                        Paint::yellow("⚠"),
                        if install.is_master {
                            format!("master/{}", install.version)
                        } else {
                            install.version.to_string()
                        }
                    );
                }

                if install.is_master {
                    // Only clear local_master_verison if we are removing the one that is tracked
                    if let Some(ref local_master) = local_master_version {
                        if install.version.to_string() == *local_master {
                            master_version_removed = true;
                        }
                    } else {
                        // if we don't know which one is local master, assume we might be removing it?
                        // or maybe we shouldn't clear it if we don't know.
                        // But if local_master_version is None, then there is nothing to clear.
                        // So master_version_removed=true is fine, clear_local_master_version handles it.
                        master_version_removed = true;
                    }
                }

                match app.toolchain_manager.delete_install(install).await {
                    Ok(()) => {
                        removed_count += 1;
                        println!(
                            "{} Removed: {}",
                            Paint::green("✓"),
                            if install.is_master {
                                format!("master/{}", install.version)
                            } else {
                                install.version.to_string()
                            }
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        eprintln!(
                            "{} Failed to remove {}: {}",
                            Paint::yellow("⚠"),
                            if install.is_master {
                                format!("master/{}", install.version)
                            } else {
                                install.version.to_string()
                            },
                            e
                        );
                    }
                }
            }
            None => {
                not_found_count += 1;
                println!("{} Version {} not found", Paint::yellow("⚠"), version);
            }
        }
    }

    if master_version_removed {
        let _ = app.toolchain_manager.clear_local_master_version();
    }

    if active_version_removed {
        handle_active_version_removal(app).await?;
    }

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

    println!("{} Cleanup completed: {}", Paint::green("ℹ"), summary);

    Ok(())
}

async fn clean_except_versions(
    app: &mut App,
    except_versions: Vec<ZigVersion>,
) -> crate::Result<()> {
    let except_versions = crate::tools::deduplicate_semver_variants(except_versions);

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

    let installations = ToolchainManager::scan_installations(&app.versions_path)?;
    let active_install = app.toolchain_manager.get_active_install().cloned();
    let mut removed_count = 0;
    let mut kept_count = 0;
    let mut failed_count = 0;
    let mut active_version_removed = false;
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
            let is_active = active_install
                .as_ref()
                .is_some_and(|active| active == install);

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

            match app.toolchain_manager.delete_install(install).await {
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

    if removed_count == 0 && failed_count == 0 {
        println!(
            "{} No cleanup needed - all installed versions were in the --except list",
            Paint::green("✓")
        );
    } else {
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

    if active_version_removed {
        handle_active_version_removal(app).await?;
    }

    Ok(())
}

async fn clean_outdated_master(app: &mut App) -> crate::Result<()> {
    println!(
        "{}",
        Paint::cyan("Removing outdated master versions...").bold()
    );

    let installations = ToolchainManager::scan_installations(&app.versions_path)?;
    let active_install = app.toolchain_manager.get_active_install().cloned();
    let mut master_installs: Vec<_> = installations
        .into_iter()
        .filter(|install| install.is_master)
        .collect();

    if master_installs.is_empty() {
        println!("{} No master versions found", Paint::yellow("⚠"));
        return Ok(());
    }

    master_installs.sort_by(|a, b| a.version.cmp(&b.version));
    let latest_master = master_installs.last().unwrap().clone();

    let mut removed_count = 0;
    let mut active_version_removed = false;

    for install in &master_installs {
        if install.version != latest_master.version {
            let is_active = active_install
                .as_ref()
                .is_some_and(|active| active == install);

            if is_active {
                active_version_removed = true;
                println!(
                    "{} Warning: Removing currently active version: master/{}",
                    Paint::yellow("⚠"),
                    install.version
                );
            }

            match app.toolchain_manager.delete_install(install).await {
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

    if active_version_removed {
        handle_active_version_removal(app).await?;
    }

    Ok(())
}

pub async fn clean_all_versions(app: &mut App) -> crate::Result<()> {
    println!("{}", Paint::cyan("Removing all versions...").bold());

    match app.toolchain_manager.delete_all_versions().await {
        Ok(()) => {
            println!(
                "{} Successfully cleaned versions directory",
                Paint::green("✓")
            );
        }
        Err(e) => {
            eprintln!(
                "{} Failed to remove versions directory: {}",
                Paint::red("✗"),
                e
            );
            return Err(e);
        }
    }

    Ok(())
}

pub async fn clean_downloads(app: &mut App) -> crate::Result<()> {
    println!("{}", Paint::cyan("Cleaning downloads directory...").bold());

    match app.toolchain_manager.clean_downloads_cache().await {
        Ok(()) => {
            println!(
                "{} Successfully cleaned downloads directory",
                Paint::green("✓")
            );
        }
        Err(e) => {
            eprintln!(
                "{} Failed to remove downloads directory: {}",
                Paint::red("✗"),
                e
            );
            return Err(e);
        }
    }

    Ok(())
}

async fn handle_active_version_removal(app: &mut App) -> crate::Result<()> {
    println!();

    let installations = ToolchainManager::scan_installations(&app.versions_path)?;

    if installations.is_empty() {
        println!(
            "{} No Zig versions remain installed. Run 'zv use <version>' to install and activate a version.",
            Paint::cyan("ℹ")
        );
        return Ok(());
    }

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

            let resolved_version = if is_master {
                ResolvedZigVersion::Master(install.version.clone())
            } else {
                ResolvedZigVersion::Semver(install.version.clone())
            };

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
