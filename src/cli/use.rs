use crate::ZigVersion;
use std::path::PathBuf;
use yansi::Paint;

use crate::{App, Result};

/// Main entry point for the use command
pub(crate) async fn use_version(version: ZigVersion, app: &mut App) -> Result<()> {
    println!(
        "{} {:#?}",
        Paint::blue("Using Zig version:").bold(),
        version
    );
    let set_zig_version = app.set_zig_version(version).await?;
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
