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
pub(crate) async fn use_version(zig_version: ZigVersion, zv: &mut App) -> Result<()> {
    let placeholder_version = SemverVersion::new(0, 0, 0);
    // First check that if version is a valid version & resolve it to a Semver i.e. ZigVersion::Semver
    let set_result = match zig_version {
        ZigVersion::Semver(ref v) => {
            if *v == placeholder_version {
                return Err(eyre!("Invalid version: {v}"));
            }
            if zv.check_installed(v, None)? {
                zv.set_active_version(&zig_version).await
            } else {
                let zig_release = zv.validate_semver(v).await?;
                let host_target = host_target(&v)
                    .ok_or_else(|| eyre!("Could not determine host target for version {v}"))?;
                if zig_release.has_target(&host_target) {
                    let install_path = zv.install_version(&zig_release.version).await?;
                    zv.set_active_version(&zig_version).await
                } else {
                    Err(eyre!("Zig version {} does not support target {}", v, host_target).into())
                }
            }
        }
        _ => todo!(),
    };
    match set_result {
        Ok(v) => {
            println!("{} {:#?}", Paint::green("✓ Set Zig version to:").bold(), v);
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}
