use crate::{
    Result,
    app::{App, CacheStrategy, ZigRelease},
};
use crate::{ZigVersion, tools};
use color_eyre::eyre::{Context, eyre};
use semver::Version as SemverVersion;
use std::path::PathBuf;
use yansi::Paint;

/// Main entry point for the use command
pub(crate) async fn use_version(zig_version: ZigVersion, app: &mut App) -> Result<()> {
    let placeholder_version = SemverVersion::new(0, 0, 0);
    // First check that if version is a valid version & resolve it to a Semver i.e. ZigVersion::Semver
    match zig_version {
        ZigVersion::Semver(ref v) => {
            if *v == placeholder_version {
                return Err(eyre!("Invalid version: {v}"));
            }
            if app.check_installed(v, None)? {
                app.set_active_version(ZigVersion::Semver(v.clone())).await?;
            } else {
                let zig_release = app.validate_semver(v).await?;
            }
        }
        _ => {}
    }
    // println!(
    //     "{} {:#?}",
    //     Paint::blue("Using Zig version:").bold(),
    //     using_version
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
