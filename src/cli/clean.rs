use crate::App;
use walkdir::WalkDir;
use yansi::Paint;

pub async fn clean(app: &App, what: Option<String>) -> crate::Result<()> {
    match what.as_deref().unwrap_or("all") {
        "bin" => clean_bin(app).await,
        "all" => clean_all(app).await,
        _ => {
            eprintln!(
                "{} Unknown clean target: {}. Use 'bin', 'versions', or 'all'.",
                Paint::red("✗"),
                what.expect("validated by unwrap_or")
            );
            Ok(())
        }
    }
}

/// Clean up executables from the bin directory, keeping only zv/zv.exe
pub async fn clean_bin(app: &App) -> crate::Result<()> {
    let bin_path = app.bin_path();
    let zv_exe_name = if cfg!(windows) { "zv.exe" } else { "zv" };

    println!("{}", Paint::cyan("Cleaning bin directory...").bold());

    if !bin_path.exists() {
        println!(
            "{} Bin directory doesn't exist: {}",
            Paint::yellow("⚠"),
            bin_path.display()
        );
        return Ok(());
    }

    let mut cleaned_count = 0;

    // Use walkdir to iterate through files in the bin directory
    for entry in WalkDir::new(&bin_path)
        .max_depth(1) // Only look at files directly in bin_path, not subdirectories
        .into_iter()
        .filter_map(|e| e.ok()) // Skip entries with errors
        .filter(|e| e.file_type().is_file())
    // Only process files
    {
        let path = entry.path();

        // Skip if it's the zv executable
        if let Some(filename) = path.file_name() {
            if filename == zv_exe_name {
                continue;
            }
        }

        // Remove the file
        match std::fs::remove_file(path) {
            Ok(()) => {
                cleaned_count += 1;
                println!(
                    "{} Removed: {}",
                    Paint::red("✗"),
                    path.file_name().unwrap().to_string_lossy()
                );
            }
            Err(e) => {
                eprintln!(
                    "{} Failed to remove {}: {}",
                    Paint::red("✗"),
                    path.display(),
                    e
                );
            }
        }
    }

    println!(
        "{} Cleaned {} executable(s) from bin directory",
        Paint::green("✓"),
        cleaned_count
    );
    Ok(())
}

/// Clean up all Zig installations from the versions directory
pub async fn clean_versions(app: &App) -> crate::Result<()> {
    let versions_path = &app.versions_path;

    println!("{}", Paint::cyan("Cleaning versions directory...").bold());

    if !versions_path.exists() {
        println!(
            "{} Versions directory doesn't exist: {}",
            Paint::yellow("⚠"),
            versions_path.display()
        );
        return Ok(());
    }

    let mut cleaned_count = 0;
    let mut failed_count = 0;

    // Use walkdir to iterate through directories in versions_path
    for entry in WalkDir::new(versions_path)
        .max_depth(2) // Look at versions/* and versions/master/*
        .min_depth(1) // Skip the versions_path itself
        .into_iter()
        .filter_map(|e| e.ok()) // Skip entries with errors
        .filter(|e| e.file_type().is_dir())
    // Only process directories
    {
        let path = entry.path();
        let depth = entry.depth();

        // Skip temporary directories (from failed installations)
        if let Some(filename) = path.file_name() {
            if filename.to_string_lossy().ends_with(".tmp") {
                continue;
            }
        }

        // We want to remove:
        // - Depth 1: versions/0.13.0, versions/0.12.0, etc. (but not versions/master)
        // - Depth 2: versions/master/0.14.0-dev.123, etc.
        let should_remove = match depth {
            1 => {
                // Don't remove the master directory itself
                path.file_name() != Some(std::ffi::OsStr::new("master"))
            }
            2 => {
                // Remove any directory inside versions/master/
                path.parent()
                    .and_then(|p| p.file_name())
                    .map(|name| name == "master")
                    .unwrap_or(false)
            }
            _ => false,
        };

        if should_remove {
            match tokio::fs::remove_dir_all(path).await {
                Ok(()) => {
                    cleaned_count += 1;
                    let display_path = if depth == 2 {
                        // For master builds, show master/version
                        format!("master/{}", path.file_name().unwrap().to_string_lossy())
                    } else {
                        // For regular builds, show just the version
                        path.file_name().unwrap().to_string_lossy().to_string()
                    };
                    println!("{} Removed: {}", Paint::red("✗"), display_path);
                }
                Err(e) => {
                    failed_count += 1;
                    eprintln!(
                        "{} Failed to remove {}: {}",
                        Paint::red("✗"),
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    if failed_count > 0 {
        println!(
            "{} Cleaned {} installation(s), {} failed",
            Paint::yellow("⚠"),
            cleaned_count,
            failed_count
        );
    } else {
        println!(
            "{} Cleaned {} Zig installation(s) from versions directory",
            Paint::green("✓"),
            cleaned_count
        );
    }

    Ok(())
}

pub fn clean_downloads(app: &App) {
    let downloads_path = app.download_cache();
    println!("{}", Paint::cyan("Cleaning downloads directory...").bold());

    match std::fs::read_dir(&downloads_path) {
        Ok(entries) => {
            let mut removed_count = 0;
            let mut failed_count = 0;

            for entry in entries.flatten() {
                let path = entry.path();

                // Special handling for tmp folder - clean its contents but keep the folder
                if path.is_dir() && path.file_name().and_then(|n| n.to_str()) == Some("tmp") {
                    for entry in WalkDir::new(&path).min_depth(1).contents_first(true) {
                        if let Ok(entry) = entry {
                            let entry_path = entry.path();
                            let result = if entry_path.is_dir() {
                                std::fs::remove_dir(entry_path)
                            } else {
                                std::fs::remove_file(entry_path)
                            };

                            match result {
                                Ok(_) => removed_count += 1,
                                Err(e) => {
                                    eprintln!(
                                        "{} Failed to remove {}: {}",
                                        Paint::red("✗"),
                                        entry_path.display(),
                                        e
                                    );
                                    failed_count += 1;
                                }
                            }
                        }
                    }
                    continue;
                }

                // Remove everything else normally
                let result = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };

                match result {
                    Ok(_) => removed_count += 1,
                    Err(e) => {
                        eprintln!(
                            "{} Failed to remove {}: {}",
                            Paint::red("✗"),
                            path.display(),
                            e
                        );
                        failed_count += 1;
                    }
                }
            }

            if failed_count == 0 {
                println!(
                    "{} Cleaned downloads directory ({} items removed)",
                    Paint::green("✓"),
                    removed_count
                );
            } else {
                println!(
                    "{} Partially cleaned downloads directory ({} removed, {} failed)",
                    Paint::yellow("⚠"),
                    removed_count,
                    failed_count
                );
            }
        }
        Err(e) => {
            eprintln!(
                "{} Failed to read downloads directory: {}",
                Paint::red("✗"),
                e
            );
        }
    }
}

/// Clean up both bin and versions directories
pub async fn clean_all(app: &App) -> crate::Result<()> {
    println!("{}", Paint::cyan("Performing full cleanup...").bold());

    clean_bin(app).await?;
    println!(); // Add spacing
    clean_versions(app).await?;
    clean_downloads(app);
    println!();
    println!("{}", Paint::green("Full cleanup completed!").bold());
    Ok(())
}
