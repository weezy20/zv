use crate::{
    Result, ZvError,
    app::{App, CacheStrategy, ZigRelease, utils::host_target},
};
use crate::{ZigVersion, tools};
use color_eyre::eyre::{Context, eyre};
use semver::Version;
use semver::Version as SemverVersion;
use std::path::PathBuf;
use yansi::Paint;

/// Main entry point for the use command
pub(crate) async fn use_version(zig_version: ZigVersion, app: &mut App) -> Result<()> {
    // Normalize ZigVersion to either Semver or one of network fetched versions Latest(_), Master(_)
    let normalized_zig_version = normalize_zig_version(zig_version, app).await?;
    let set_result = match normalized_zig_version {
        zv_semver @ ZigVersion::Semver(_) => 'semver: {
            let release = app
                .validate_semver(zv_semver.version().expect("Valid semver"))
                .await?;
            if app.check_installed(release.version(), None) {
                break 'semver app.set_active_version(&zv_semver).await?.to_owned();
            } else {
                app.install_release(&release).await.wrap_err_with(|| {
                    format!(
                        "Failed to download and install Zig version {}",
                        zv_semver.version().expect("Valid semver")
                    )
                })?;
                break 'semver app.set_active_version(&zv_semver).await?.to_owned();
            }
        }
        _ => todo!(),
    };
    println!("✅ Active zig version set: {}", Paint::blue(&set_result));
    Ok(())
}

/// Accepts ZigVersion constructed from the command line and normalizes it to either:
/// - Semver(v) - for concrete versions, stable/latest resolved from network
/// - Latest(None) - if latest version could not be resolved from network
/// - Master(None) - if master version could not be resolved from network
async fn normalize_zig_version(zig_version: ZigVersion, app: &mut App) -> Result<ZigVersion> {
    Ok(match zig_version {
        ZigVersion::Semver(ref v) if *v == SemverVersion::new(0, 0, 0) => {
            return Err(eyre!("Invalid version: {v}"));
        }
        // Direct semver version - use as-is
        zv @ ZigVersion::Semver(_) => zv,

        // Unresolved stable - fetch from network/index
        ZigVersion::Stable(None) => 'stable: {
            let stable_version = app.fetch_latest_version(CacheStrategy::RespectTtl).await;
            if let Err(err) = stable_version {
                tracing::error!("Failed to fetch stable version from index: {err}");
                break 'stable ZigVersion::Stable(None);
            }
            let stable_version_from_release = stable_version.unwrap();
            break 'stable ZigVersion::Semver(
                semver::Version::parse(stable_version_from_release.version())
                    .wrap_err_with(|| "Failed to parse ZigRelease version as semver")?,
            );
        }

        // Already resolved stable - convert to Semver
        ZigVersion::Stable(Some(v)) => ZigVersion::Semver(v),

        // Handle latest versions: try to resolve from network, else fallback to None
        ZigVersion::Latest(None) => 'latest: {
            let latest_version = app.fetch_latest_version(CacheStrategy::AlwaysRefresh).await;
            if let Err(err) = latest_version {
                tracing::error!("Failed to fetch latest version from index: {err}");
                break 'latest ZigVersion::Latest(None);
            }
            let latest_version_from_release = latest_version.unwrap();
            break 'latest ZigVersion::Semver(
                semver::Version::parse(latest_version_from_release.version())
                    .wrap_err_with(|| "Failed to parse ZigRelease version as semver")?,
            );
        }
        // Handle master versions: try to resolve from network, else fallback to None
        ZigVersion::Master(None) => 'master: {
            if let Ok(mv) = app.fetch_master_version().await {
                let master_version = SemverVersion::parse(mv.version())
                    .wrap_err_with(|| "Failed to parse ZigRelease version as semver")?;
                break 'master ZigVersion::Master(Some(master_version));
            } else {
                tools::error(
                    "Failed to fetch master version from network.. Falling back to locally available master version",
                );
                ZigVersion::Master(None)
            }
        }
        ZigVersion::Latest(Some(_)) => {
            unreachable!("Impossible to construct latest with concrete version")
        }
        ZigVersion::Master(Some(_)) => {
            unreachable!("Impossible to construct master with concrete version")
        }
    })
}
