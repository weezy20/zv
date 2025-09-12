use crate::{Result, app::App};
use crate::{ZigVersion, tools};
use color_eyre::eyre::{Result, eyre};
use semver::Version as SemverVersion;
use std::path::PathBuf;
use yansi::Paint;

/// Main entry point for the use command
pub(crate) async fn use_version(zig_version: ZigVersion, app: &mut App) -> Result<()> {
    let placeholder_version = SemverVersion::new(0, 0, 0);
    // First check that if version is a valid version & resolve it to a Semver i.e. ZigVersion::Semver
    let using_version: ZigVersion = match zig_version {
        ZigVersion::Semver(ref v) => {
            if *v == placeholder_version {
                return Err(eyre!("Invalid semver version: {}", v));
            } else {
                ZigVersion::Semver(*v)
            }
        }
        ZigVersion::Master(ref v) => {
            assert!(
                *v == placeholder_version,
                "Impossible to construct Master with non-placeholder version"
            );
            app.fetch_master_version().await?
        }
        ZigVersion::Stable(ref v) => {
            if *v == placeholder_version {
                app.fetch_stable_version().await?
            } else {
                ZigVersion::Semver(*v)
            }
        }
        ZigVersion::Latest(ref v) => {
            assert!(
                *v == placeholder_version,
                "Impossible to construct Latest with non-placeholder version"
            );
            app.fetch_latest_version().await?
        }
    };
    // println!(
    //     "{} {:#?}",
    //     Paint::blue("Using Zig version:").bold(),
    //     version
    // );
    // let set_zig_version = app.set_active_version(version).await?;
    // println!(
    //     "{} {:#?}",
    //     Paint::green("✓ Set Zig version to:").bold(),
    //     set_zig_version
    // );
    // todo!(
    //     "impl use for system, system@<version>, system@<version> --path=<path>, --path=<path>, latest, master, stable, <version>"
    // );
    Ok(())
}
