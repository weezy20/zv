use crate::{ResolvedZigVersion, ZigVersion, tools};
use crate::{
    Result, ZvError,
    app::{
        App, CacheStrategy,
        network::{ZigIndex, ZigRelease},
        utils::host_target,
    },
};
use color_eyre::eyre::{Context, eyre};
use semver::Version;
use semver::Version as SemverVersion;
use std::path::PathBuf;
use yansi::Paint;

/// Main entry point for the use command
pub(crate) async fn use_version(
    zig_version: ZigVersion,
    app: &mut App,
    force_ziglang: bool,
) -> Result<()> {
    // Resolve ZigVersion to a validated ResolvedZigVersion
    // This already does all the validation and fetching we need
    let resolved_version = resolve_zig_version(app, &zig_version).await
        .map_err(|e| {
            match e {
                ZvError::ZigVersionResolveError(err) => {
                    ZvError::ZigVersionResolveError(eyre!(
                        "Failed to resolve version '{}': {}. Try running 'zv sync' to update the index or 'zv list' to see available versions.",
                        zig_version, err
                    ))
                }
                _ => e,
            }
        })?;

    if let Some(p) = app.check_installed(&resolved_version) {
        // Version is already installed, just set it as active
        app.set_active_version(&resolved_version, Some(p)).await?
    } else {
        app.install_release(force_ziglang).await.wrap_err_with(|| {
            format!(
                "Failed to download and install Zig version {}",
                resolved_version
            )
        })?;

        app.set_active_version(&resolved_version, None).await?
    }

    println!(
        "✅ Active zig version set: {}",
        Paint::blue(&resolved_version.version().to_string())
    );
    Ok(())
}

/// Resolves a ZigVersion against the app's index using network operations when needed
///
/// # Arguments
///
/// * `app` - Mutable reference to the App instance
/// * `version` - The ZigVersion to resolve
///
/// # Returns
///
/// * `Ok(ResolvedZigVersion)` - If the version was successfully resolved and validated
/// * `Err(ZvError)` - If the version cannot be resolved or validation fails
pub async fn resolve_zig_version(
    app: &mut App,
    version: &ZigVersion,
) -> Result<ResolvedZigVersion, ZvError> {
    match version {
        // Direct semver - validate it exists using app.validate_semver()
        ZigVersion::Semver(v) => {
            let zig_release = app.validate_semver(v).await?;
            app.to_install = Some(zig_release);
            Ok(ResolvedZigVersion::Semver(v.clone()))
        }

        // Master with specific version - fetch master and verify it matches
        ZigVersion::Master(Some(v)) => {
            let master_release = app.fetch_master_version().await?;
            let master_version = master_release.resolved_version();

            // Extract the semver version from the resolved version for comparison
            let index_master_version = match master_version {
                ResolvedZigVersion::Semver(semver) => semver,
                ResolvedZigVersion::Master(semver) => semver,
            };

            // Verify the requested version matches the actual master version
            if index_master_version == v {
                app.to_install = Some(master_release);
                Ok(ResolvedZigVersion::Master(v.clone()))
            } else {
                Err(ZvError::ZigVersionResolveError(eyre!(
                    "Master version mismatch: requested {}, but master is {}",
                    v,
                    index_master_version
                )))
            }
        }

        // Master without version - fetch current master
        ZigVersion::Master(None) => {
            let master_release = app.fetch_master_version().await?;
            let master_version = master_release.resolved_version().clone();

            // Extract the concrete version from the master release
            match master_version {
                ResolvedZigVersion::Master(v) => {
                    app.to_install = Some(master_release);
                    Ok(ResolvedZigVersion::Master(v))
                }
                ResolvedZigVersion::Semver(v) => {
                    // If master is returned as a semver, convert it to MasterVersion
                    app.to_install = Some(master_release);
                    Ok(ResolvedZigVersion::Master(v))
                }
            }
        }

        // Stable with specific version - validate it's stable and exists
        ZigVersion::Stable(Some(v)) => {
            // Verify the version is actually stable (no pre-release or build metadata)
            if !v.pre.is_empty() || !v.build.is_empty() {
                return Err(ZvError::ZigVersionResolveError(eyre!(
                    "Version {} is not stable (contains pre-release or build metadata)",
                    v
                )));
            }

            // Validate the version exists using RespectTTL strategy
            let _zig_release = app.validate_semver(v).await?;
            Ok(ResolvedZigVersion::Semver(v.clone()))
        }

        // Stable without version - fetch latest stable version
        ZigVersion::Stable(None) => {
            // Use RespectTTL strategy for stable versions
            let stable_release = app.fetch_latest_version(CacheStrategy::RespectTtl).await?;
            let stable_version = stable_release.resolved_version();

            // Extract the semver from the resolved version
            match stable_version {
                ResolvedZigVersion::Semver(semver) => {
                    Ok(ResolvedZigVersion::Semver(semver.clone()))
                }
                _ => Err(ZvError::ZigVersionResolveError(eyre!(
                    "Latest stable version is not a semver release"
                ))),
            }
        }

        // Latest with specific version - validate it exists (no stability check)
        ZigVersion::Latest(Some(v)) => {
            let _zig_release = app.validate_semver(v).await?;
            Ok(ResolvedZigVersion::Semver(v.clone()))
        }

        // Latest without version - fetch latest stable version with AlwaysRefresh
        ZigVersion::Latest(None) => {
            // Use AlwaysRefresh strategy for latest versions
            let latest_release = app
                .fetch_latest_version(CacheStrategy::AlwaysRefresh)
                .await?;
            let latest_version = latest_release.resolved_version();

            // Extract the semver from the resolved version
            match latest_version {
                ResolvedZigVersion::Semver(semver) => {
                    Ok(ResolvedZigVersion::Semver(semver.clone()))
                }
                _ => Err(ZvError::ZigVersionResolveError(eyre!(
                    "Latest version is not a semver release"
                ))),
            }
        }
    }
}
