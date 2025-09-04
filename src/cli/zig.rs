use color_eyre::eyre::{bail, eyre};
use std::process::{Command, Stdio};

const MAX_RECURSION: u32 = 10;

pub fn zig_main() -> crate::Result<()> {
    // Recursion guard
    let recursion_count: u32 = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Default to any zig bin for now
    let zig_path = which::which("zig").map_err(|_| eyre!("Could not find system zig"))?;

    let mut args = std::env::args_os();
    args.next(); // drop program name

    let mut child = Command::new(zig_path)
        .args(args)
        .env("ZV_RECURSION_COUNT", (recursion_count + 1).to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| eyre!("Failed to launch real zig: {}", e))?;

    let status = child
        .wait()
        .map_err(|e| eyre!("Failed to wait for zig: {}", e))?;

    std::process::exit(status.code().unwrap_or(1));
}
