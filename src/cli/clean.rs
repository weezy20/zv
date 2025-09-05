use crate::App;
use color_eyre::eyre::{Context as _};
use yansi::Paint;
use walkdir::WalkDir;

/// Clean up executables from the bin directory, keeping only zv/zv.exe
pub async fn clean_bin(app: &App) -> crate::Result<()> {
    let bin_path = app.bin_path();
    let zv_exe_name = if cfg!(windows) { "zv.exe" } else { "zv" };
    
    println!("{}", Paint::cyan("Cleaning bin directory...").bold());
    
    if !bin_path.exists() {
        println!("{} Bin directory doesn't exist: {}", Paint::yellow("⚠"), bin_path.display());
        return Ok(());
    }
    
    let mut cleaned_count = 0;
    
    // Use walkdir to iterate through files in the bin directory
    for entry in WalkDir::new(&bin_path)
        .max_depth(1) // Only look at files directly in bin_path, not subdirectories
        .into_iter()
        .filter_map(|e| e.ok()) // Skip entries with errors
        .filter(|e| e.file_type().is_file()) // Only process files
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
                println!("{} Removed: {}", Paint::red("✗"), path.file_name().unwrap().to_string_lossy());
            }
            Err(e) => {
                eprintln!("{} Failed to remove {}: {}", Paint::red("✗"), path.display(), e);
            }
        }
    }
    
    println!("{} Cleaned {} executable(s) from bin directory", Paint::green("✓"), cleaned_count);
    Ok(())
}
