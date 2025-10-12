use crate::cli::ZlsCmd;
use crate::{App, UserConfig, ZvError, tools};
use color_eyre::eyre::eyre;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub async fn zls_main() -> crate::Result<()> {
    // Recursion guard - check early to prevent infinite loops
    crate::check_recursion_with_context("zls proxy")?;

    // Collect command line arguments
    let mut args: Vec<String> = std::env::args().collect();
    args.remove(0); // drop program name

    let zls_path = find_local_compatible_zls().await?;

    // Get current recursion count for incrementing
    let recursion_count: u32 = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let mut child = Command::new(zls_path)
        .args(args)
        .env("ZV_RECURSION_COUNT", (recursion_count + 1).to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| eyre!("Failed to launch ZLS: {}", e))?;

    let status = child
        .wait()
        .map_err(|e| eyre!("Failed to wait for ZLS: {}", e))?;

    std::process::exit(status.code().unwrap_or(1));
}

/// Find a compatible ZLS executable for the current Zig version
async fn find_local_compatible_zls() -> Result<PathBuf, ZvError> {
    // Initialize app to access zv directory structure
    let (zv_base_path, _) = tools::fetch_zv_dir()?;

    let mut app = App::init(UserConfig {
        zv_base_path,
        shell: None,
    })
    .await
    .map_err(|e| eyre!("Failed to initialize app: {}", e))?;

    app.zls_for_current_active_zig().await
}

pub(crate) async fn zls_command(cmd: ZlsCmd, mut app: App) -> crate::Result<()> {
    Ok(())
}
