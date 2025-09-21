use crate::{ResolvedZigVersion, ZigVersion, tools};
use crate::{
    Result, ZvError,
    app::{App, CacheStrategy, ZigRelease, network::ZigIndex, utils::host_target},
};
use color_eyre::eyre::{Context, eyre};
use semver::Version;
use semver::Version as SemverVersion;
use std::path::PathBuf;
use yansi::Paint;

/// Main entry point for the use command
pub(crate) async fn use_version(zig_version: ZigVersion, app: &mut App) -> Result<()> {
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

    // Create a version string for installation checking
    let (version_string, nesting) = match &resolved_version {
        ResolvedZigVersion::Semver(v) => (v.to_string(), None),
        ResolvedZigVersion::Master(v) => (v.to_string(), None),
    };

    let set_result = if app.check_installed(&version_string, nesting) {
        // Version is already installed, just set it as active
        app.set_active_version(&resolved_version).await?.to_string()
    } else {
        app.install_release().await.wrap_err_with(|| {
            format!(
                "Failed to download and install Zig version {}",
                resolved_version
            )
        })?;

        app.set_active_version(&resolved_version).await?.to_string()
    };

    println!("✅ Active zig version set: {}", Paint::blue(&set_result));
    Ok(())
}

/// Performs local-only resolution of ZigVersion against the current index
///
/// It handles all ZigVersion variants and converts them to ResolvedZigVersion if they exist
/// in the index.
///
/// # Arguments
///
/// * `version` - The ZigVersion to resolve
/// * `index` - The ZigIndex to resolve against
///
/// # Returns
///
/// * `Some(ResolvedZigVersion)` - If the version exists in the index
/// * `None` - If the version cannot be resolved against the index
///
/// # Examples
///
/// ```rust
/// use crate::app::version_resolution::normalize_zig_version;
/// use crate::types::{ZigVersion, ResolvedZigVersion};
/// use semver::Version;
///
/// let version = ZigVersion::Semver(Version::parse("0.11.0").unwrap());
/// let resolved = normalize_zig_version(&version, &index);
/// ```
pub fn normalize_zig_version(version: &ZigVersion, index: &ZigIndex) -> Option<ResolvedZigVersion> {
    match version {
        // Direct semver - check if it exists in the index
        ZigVersion::Semver(v) => {
            let resolved = ResolvedZigVersion::Semver(v.clone());
            if index.has_version(&resolved) {
                Some(resolved)
            } else {
                None
            }
        }

        // Master with specific version - verify it matches the index
        ZigVersion::Master(Some(v)) => {
            let resolved = ResolvedZigVersion::Master(v.clone());
            if index.has_version(&resolved) {
                Some(resolved)
            } else {
                None
            }
        }

        // Master without version - look for any master version in index
        ZigVersion::Master(None) => {
            // Find any master version in the index
            index.releases().keys().find_map(|version| {
                match version {
                    ResolvedZigVersion::Master(_) => Some(version.clone()),
                    _ => None,
                }
            })
        }

        // Stable with specific version - verify it's stable and exists
        ZigVersion::Stable(Some(v)) => {
            // Verify the version is actually stable (no pre-release or build metadata)
            if !v.pre.is_empty() || !v.build.is_empty() {
                return None;
            }

            let resolved = ResolvedZigVersion::Semver(v.clone());
            if index.has_version(&resolved) {
                Some(resolved)
            } else {
                None
            }
        }

        // Stable without version - find highest stable version in index
        ZigVersion::Stable(None) => find_highest_stable_version(index),

        // Latest with specific version - verify it exists (no stability check)
        ZigVersion::Latest(Some(v)) => {
            let resolved = ResolvedZigVersion::Semver(v.clone());
            if index.has_version(&resolved) {
                Some(resolved)
            } else {
                None
            }
        }

        // Latest without version - find highest stable version in index
        ZigVersion::Latest(None) => find_highest_stable_version(index),
    }
}

/// Resolves a ZigVersion against the app's index using network operations when needed
///
/// This function validates user input by using existing app methods to fetch and validate
/// versions. It applies appropriate cache strategies based on the version type and integrates
/// with the app's network layer for validation.
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
///
/// # Cache Strategies Applied
///
/// * Semver/Stable with version: RespectTTL, fallback to OnlyCache
/// * Latest: AlwaysRefresh, fallback to latest from OnlyCache
/// * Master: AlwaysRefresh, fallback to OnlyCache
///
/// # Example usage:
///
/// ```rust
/// let version = ZigVersion::Semver(Version::parse("0.11.0").unwrap());
/// let resolved = resolve_zig_version(&mut app, &version).await?;
/// ```
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

/// Helper function to find the highest stable version in the index
///
/// A stable version is defined as a semantic version without pre-release identifiers
/// or build metadata. This function iterates through all ResolvedZigVersion::Semver
/// variants in the index and returns the highest stable one.
///
/// # Arguments
///
/// * `index` - The ZigIndex to search
///
/// # Returns
///
/// * `Some(ResolvedZigVersion::Semver)` - The highest stable version found
/// * `None` - If no stable versions exist in the index
fn find_highest_stable_version(index: &ZigIndex) -> Option<ResolvedZigVersion> {
    index
        .releases()
        .keys()
        .filter_map(|resolved_version| {
            match resolved_version {
                ResolvedZigVersion::Semver(v) => {
                    // Only consider stable releases (no pre-release or build metadata)
                    if v.pre.is_empty() && v.build.is_empty() {
                        Some(resolved_version.clone())
                    } else {
                        None
                    }
                }
                // Master variants are not considered stable
                _ => None,
            }
        })
        .max() // BTreeMap keys are ordered, so max() gives us the highest version
}
