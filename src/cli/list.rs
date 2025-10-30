use crate::{App, Result};
use semver::Version;
use yansi::Paint;

const SEPARATOR: &str = "\n----------------------------------------\n";

pub async fn list_opts(mut app: App, all: bool, mirrors: bool, refresh: bool) -> Result<()> {
    if !all && !mirrors {
        list_versions(&app).await
    } else if all && mirrors {
        let mut app = list_all(app, refresh).await?;
        println!("{SEPARATOR}");
        let _ = list_mirrors(&mut app, refresh).await?;
        Ok(())
    } else if all {
        list_all(app, refresh).await.and_then(|_| Ok(()))
    } else if mirrors {
        list_mirrors(&mut app, refresh).await
    } else {
        Ok(())
    }
}
pub async fn list_versions(app: &App) -> Result<()> {
    let installed = app.toolchain_manager.list_installations();

    if installed.is_empty() {
        println!("{}", "No zig versions installed.".italic());
        return Ok(());
    }

    println!("{}", "Installed zig versions:".italic());

    // Get terminal width, default to 80 if unable to determine
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);

    let target_width = (term_width as f32 * 0.6) as usize;
    let mut current_line_width = 0;
    let mut is_first = true;

    for (version, is_active, is_master) in installed.iter() {
        let active_marker = if *is_active {
            Paint::green("★ ").to_string()
        } else {
            "  ".into()
        };

        let master_marker = if *is_master {
            Paint::yellow(" (master)").to_string()
        } else {
            "".into()
        };

        let version_display = if *is_active {
            Paint::green(&version.to_string()).bold().to_string()
        } else {
            version.to_string()
        };

        let full_item = format!("{}{}{}", active_marker, version_display, master_marker);

        // Calculate visible width (approximate, not accounting for ANSI codes)
        let visible_width = version.to_string().len() + 2 + master_marker.len(); // +2 for active_marker space
        let item_width = visible_width + 3; // +3 for separator padding

        // Check if adding this version would exceed target width
        if !is_first && current_line_width + item_width > target_width {
            println!(); // Start new line
            current_line_width = 0;
        }

        if current_line_width == 0 {
            print!("{}", full_item);
            current_line_width = visible_width;
        } else {
            print!(",  {}", full_item);
            current_line_width += item_width;
        }

        is_first = false;
    }

    println!(); // Final newline

    Ok(())
}
async fn list_all(mut app: App, refresh: bool) -> Result<App> {
    let installed = app
        .toolchain_manager
        .list_installations()
        .iter()
        .map(|i| i.0.clone())
        .collect::<Vec<Version>>();

    let cache_strategy = if refresh {
        crate::app::CacheStrategy::AlwaysRefresh
    } else {
        crate::app::CacheStrategy::PreferCache
    };

    let index = app.index_manager().await?;
    let zig_index = index.ensure_loaded(cache_strategy).await?;

    // Get terminal width, default to 80 if unable to determine
    let term_width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);

    let target_width = (term_width as f32 * 0.6) as usize;
    let mut current_line_width = 0;
    let mut is_first = true;

    println!("{}\n", "Available zig versions in cached index:".italic());
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

async fn list_mirrors(app: &mut App, refresh: bool) -> Result<()> {
    let cache_strategy = if refresh {
        crate::app::CacheStrategy::AlwaysRefresh
    } else {
        crate::app::CacheStrategy::PreferCache
    };

    // Get the mirror manager and load mirrors using the appropriate strategy
    let mirror_manager = app.mirror_manager().await?;

    // Load mirrors with the selected cache strategy
    mirror_manager
        .load_mirrors(cache_strategy)
        .await
        .map_err(crate::ZvError::NetworkError)?;

    // Get all mirrors and sort by rank
    let mirrors = mirror_manager
        .sort_by_rank()
        .await
        .map_err(crate::ZvError::NetworkError)?;

    if mirrors.is_empty() {
        println!("{}", "No community mirrors available.".italic());
        return Ok(());
    }

    println!("{}", "Community mirrors:".italic());
    println!();

    // Display each mirror with rank and URL
    for mirror in mirrors.iter() {
        let rank_str = format!("#{}", mirror.rank);
        let rank_display = match mirror.rank {
            1 => Paint::green(&rank_str).bold().to_string(),
            2..=3 => Paint::yellow(&rank_str).to_string(),
            _ => Paint::red(&rank_str).to_string(),
        };

        let layout_display = match mirror.layout {
            crate::app::network::mirror::Layout::Flat => "flat",
            crate::app::network::mirror::Layout::Versioned => "versioned",
        };

        println!(
            "  {} {} ({})",
            rank_display,
            mirror.base_url,
            Paint::cyan(layout_display).italic()
        );
    }
    println!();
    println!(
        "{}",
        "Lower rank numbers indicate higher priority mirrors."
            .italic()
            .dim()
    );

    let zv_dir_display = match crate::tools::fetch_zv_dir() {
        Ok((path, _)) => path.display().to_string(),
        Err(_) => "ZV_DIR".to_string(),
    };

    println!(
        "{}",
        format!("You can edit mirror rankings in your {zv_dir_display}/mirrors.toml file.")
            .italic()
            .dim()
    );

    Ok(())
}
