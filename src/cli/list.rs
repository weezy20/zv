use std::time::Duration;

use crate::{App, Result};
use semver::Version;
use yansi::Paint;

const SEPARATOR: &str = "\n----------------------------------------\n";

pub async fn list_opts(mut app: App, all: bool, mirrors: bool) -> Result<()> {
    if !all && !mirrors {
        list_versions(&app).await
    } else if all && mirrors {
        let mut app = list_all(app).await?;
        println!("{SEPARATOR}");
        let _ = list_mirrors(&mut app).await?;
        Ok(())
    } else if all {
        list_all(app).await.and_then(|_| Ok(()))
    } else if mirrors {
        list_mirrors(&mut app).await
    } else {
        Ok(())
    }
}

pub async fn list_versions(app: &App) -> Result<()> {
    let installed = app.toolchain_manager.list_installations();
    println!("{}","Installed zig versions:".italic());
    for (version, is_active, is_master) in installed.iter() {
        let active_marker = if *is_active {
            Paint::green("★ ").to_string()
        } else {
            "  ".into()
        };

        let master_marker = if *is_master {
            Paint::yellow(" (master)").to_string()
        } else {
            "  ".into()
        };

        let version_display = if *is_active {
            Paint::green(&version.to_string()).bold().to_string()
        } else {
            version.to_string()
        };

        println!("{active_marker}{version_display}{master_marker}");
    }

    Ok(())
}

async fn list_all(mut app: App) -> Result<App> {
    let installed = app
        .toolchain_manager
        .list_installations()
        .iter()
        .map(|i| i.0.clone())
        .collect::<Vec<Version>>();

    let index = app.index_manager().await?;
    let zig_index = index
        .ensure_loaded(crate::app::CacheStrategy::PreferCache)
        .await?;

    // Get terminal width, default to 80 if unable to determine
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);

    let target_width = (term_width as f32 * 0.6) as usize;
    let mut current_line_width = 0;
    let mut is_first = true;

    println!("{}","Available zig versions in cached index:".italic());
    for version in zig_index.releases().keys().rev() {
        let version_str = if installed.contains(version.version()) {
            format!("{}", Paint::green(version).bold())
        } else {
            format!("{}", version)
        };
        let item_width = version_str.len() + 3; // +3 for ", " separator and padding

        // Check if adding this version would exceed target width
        if !is_first && current_line_width + item_width > target_width {
            println!(); // Start new line
            current_line_width = 0;
        }

        if current_line_width == 0 {
            print!("{}", version_str);
            current_line_width = version_str.len();
        } else {
            print!(",  {}", version_str);
            current_line_width += item_width;
        }

        is_first = false;
    }

    println!(); // Final newline

    Ok(app)
}

async fn list_mirrors(app: &mut App) -> Result<()> {
    todo!()
}
