use crate::App;
use tokio::fs;
use yansi::Paint;

pub async fn uninstall(app: &mut App) -> crate::Result<()> {
    let zv_dir = app.path();
    let bin_path = app.bin_path();

    println!("{}", Paint::red("Uninstalling zv...").bold());
    println!();

    if !zv_dir.exists() {
        println!(
            "{} zv directory does not exist: {}",
            Paint::yellow("⚠"),
            zv_dir.display()
        );
        return Ok(());
    }

    println!(
        "{} zv directory detected: {}",
        Paint::cyan("→"),
        zv_dir.display()
    );

    match fs::remove_dir_all(zv_dir).await {
        Ok(()) => {
            println!("{} Successfully removed zv directory", Paint::green("✓"));
        }
        Err(e) => {
            return Err(color_eyre::eyre::eyre!(
                "Failed to remove zv directory {}: {}",
                zv_dir.display(),
                e
            ));
        }
    }

    println!();

    if app.source_set {
        println!(
            "{}",
            Paint::yellow("⚠ Important: PATH cleanup needed").bold()
        );
        println!(
            "Remove {} from your PATH environment variable for a full cleanup.",
            Paint::yellow(&bin_path.display().to_string())
        );
    }

    println!();
    println!(
        "{}",
        Paint::green("zv has been uninstalled successfully!").bold()
    );

    Ok(())
}
