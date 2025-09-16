use crate::{
    Result,
    app::{App, CacheStrategy},
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
    let using_version: ZigVersion = match zig_version {
        ZigVersion::Semver(ref v) => {
            if *v == placeholder_version {
                return Err(eyre!("Invalid semver version: {}", v));
            } else {
                let zv = app
                    .validate_semver(v)
                    .await
                    .wrap_err_with(|| format!("Invalid semver version: {}", v))?;
                zv
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
                app.fetch_latest_version(CacheStrategy::RespectTtl).await?
            } else {
                let zv = app
                    .validate_semver(v)
                    .await
                    .wrap_err_with(|| format!("Invalid semver version: {}", v))?;
                zv
            }
        }
        ZigVersion::Latest(ref v) => {
            assert!(
                *v == placeholder_version,
                "Impossible to construct Latest with non-placeholder version"
            );
            app.fetch_latest_version(CacheStrategy::AlwaysRefresh)
                .await?
        }
    };
    println!(
        "{} {:#?}",
        Paint::blue("Using Zig version:").bold(),
        using_version
    );
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
