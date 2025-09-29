use crate::{App, Result};
use yansi::Paint;

pub async fn list_versions(app: &mut App) -> Result<()> {
    let installed = app.toolchain_manager.list_installations();

    for (version, is_active, is_master) in installed {
        let active_marker = if is_active {
            Paint::green("★ ").to_string()
        } else {
            "  ".into()
        };

        let master_marker = if is_master {
            Paint::yellow(" (master)").to_string()
        } else {
            "  ".into()
        };

        let version_display = if is_active {
            Paint::green(&version.to_string()).bold().to_string()
        } else {
            version.to_string()
        };

        println!("{active_marker}{version_display}{master_marker}");
    }

    Ok(())
}
