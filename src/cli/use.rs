use crate::{
    Result, ZvError,
    app::{App, CacheStrategy, ZigRelease, utils::host_target},
};
use crate::{ZigVersion, tools};
use color_eyre::eyre::{Context, eyre};
use semver::Version as SemverVersion;
use std::path::PathBuf;
use yansi::Paint;

/// Main entry point for the use command
pub(crate) async fn use_version(zig_version: ZigVersion, app: &mut App) -> Result<()> {
    let normalized_zig_version = match zig_version {
        ZigVersion::Semver(ref v) if *v == SemverVersion::new(0, 0, 0) => {
            return Err(eyre!("Invalid version: {v}"));
        }
        // Direct semver version - use as-is
        zv @ ZigVersion::Semver(_)  => zv,

        // Unresolved stable - fetch from network/index
        ZigVersion::Stable(None) => {
            let stable_version = app.fetch_latest_version(CacheStrategy::RespectTtl).await;
            if let Err(err) = stable_version {
                tracing::error!("Failed to fetch stable version from index: {err}");
                todo!("Implement fallback to using locally install highest stable version");
            }
            let stable_version = stable_version.unwrap();
            ZigVersion::Semver(
                semver::Version::parse(stable_version.version.as_str())
                    .wrap_err_with(|| "Failed to parse ZigRelease version as semver")?,
            )
        }

        // Already resolved stable - convert to Semver
        ZigVersion::Stable(Some(v)) => ZigVersion::Semver(v),

        // Handle latest versions
        zv @ ZigVersion::Latest(None) => zv,
        ZigVersion::Latest(Some(_)) => {
            unreachable!("Impossible to construct latest with concrete version")
        }

        // Handle master versions
        zv @ ZigVersion::Master(None) => zv,
        ZigVersion::Master(Some(_)) => {
            unreachable!("Impossible to construct master with concrete version")
        }
    };

    println!("Using Zig version: {normalized_zig_version}");
    // First check that if version is a valid version & resolve it to a Semver i.e. ZigVersion::Semver
    // let set_result = match zig_version {
    //     ZigVersion::Semver(ref v) => {
    //         if *v == placeholder_version {
    //             return Err(eyre!("Invalid version: {v}"));
    //         }
    //         if zv.check_installed(v, None)? {
    //             zv.set_active_version(&zig_version).await
    //         } else {
    //             let zig_release = zv.validate_semver(v).await?;
    //             let host_target = host_target(&v)
    //                 .ok_or_else(|| eyre!("Could not determine host target for version {v}"))?;
    //             if zig_release.has_target(&host_target) {
    //                 let install_path = zv.install_version(&zig_release.version).await?;
    //                 zv.set_active_version(&zig_version).await
    //             } else {
    //                 Err(eyre!("Zig version {} does not support target {}", v, host_target).into())
    //             }
    //         }
    //     }
    //     _ => todo!(),
    // };
    // match set_result {
    //     Ok(v) => {
    //         println!("{} {:#?}", Paint::green("✓ Set Zig version to:").bold(), v);
    //         Ok(())
    //     }
    //     Err(e) => Err(e.into()),
    // }
    Ok(())
}
