use crate::ZigVersion;
use std::path::PathBuf;
use yansi::Paint;

use crate::{Result, app::App};

/// Main entry point for the use command
pub(crate) async fn use_version(version: ZigVersion, app: &mut App) -> Result<()> {
    // First check that if version is a valid version
    let resolved_version = if version.is_placeholder_version() {

    }
    println!(
        "{} {:#?}",
        Paint::blue("Using Zig version:").bold(),
        version
    );
    let set_zig_version = app.set_active_version(version).await?;
    println!(
        "{} {:#?}",
        Paint::green("✓ Set Zig version to:").bold(),
        set_zig_version
    );
    // todo!(
    //     "impl use for system, system@<version>, system@<version> --path=<path>, --path=<path>, latest, master, stable, <version>"
    // );
    Ok(())
}
