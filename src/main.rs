#![allow(unused, warnings)]

use std::borrow::Cow;

use color_eyre::{
    Result,
    config::{HookBuilder, Theme},
    eyre::Context,
};

// We only expect to route to `zig` or `zls` once from `zv`
// For example: `zv init --zig`  => `zv` spawns `zig`, +1 in [instantiate_zig]
const ZV_RECURSION_MAX: u32 = 1;

#[tokio::main]
async fn main() -> Result<()> {
    check_recursion()?;

    yansi::whenever(yansi::Condition::TTY_AND_COLOR);
    if yansi::is_enabled() {
        color_eyre::install()?;
    } else {
        HookBuilder::default().theme(Theme::new()).install()?;
    }

    #[cfg(feature = "dotenv")]
    dotenv::dotenv().ok();

    #[cfg(feature = "log")]
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "zv=info".into()))
        .with_writer(std::io::stderr)
        .init();

    #[cfg(windows)]
    apply_windows_security_mitigations();

    let program_name = get_program_name()?;

    match program_name.as_str() {
        "zv" => cli::zv_main().await,
        "zig" => cli::zig_main(),
        "zls" => cli::zls_main(),
        _ => {
            eprintln!(
                "Unknown invocation: {}. This binary should be invoked as 'zv', 'zig', or 'zls'.",
                program_name
            );
            std::process::exit(1);
        }
    }
}

fn get_program_name() -> Result<String> {
    let current_exe = std::env::current_exe().wrap_err("Failed to get current executable path")?;

    let file_name = current_exe
        .file_name()
        .ok_or_else(|| color_eyre::eyre::eyre!("Failed to get executable filename"))?
        .to_string_lossy();

    // Remove .exe extension on Windows
    let name = if cfg!(windows) && file_name.ends_with(".exe") {
        file_name.strip_suffix(".exe").unwrap().to_string()
    } else {
        file_name.to_string()
    };
    Ok(name)
}

#[cfg(windows)]
pub fn apply_windows_security_mitigations() {
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_SYSTEM32, LOAD_LIBRARY_SEARCH_USER_DIRS, SetDefaultDllDirectories,
    };

    // Restrict DLL loading to system directories only
    // This prevents loading DLLs from the current directory or PATH
    let search_flags = LOAD_LIBRARY_SEARCH_SYSTEM32 | LOAD_LIBRARY_SEARCH_USER_DIRS;

    unsafe {
        let result = SetDefaultDllDirectories(search_flags);
        // SetDefaultDllDirectories should never fail with valid arguments
        assert_ne!(result, 0, "Failed to set secure DLL directories");
    }
}

fn check_recursion() -> Result<()> {
    // Recursion guard - prevent infinite loops but allow zig subcommands such as zv init --zig :  zv -> zig
    let recursion_count = std::env::var("ZV_RECURSION_COUNT")
        .unwrap_or_else(|_| "0".to_string())
        .parse::<u32>()
        .unwrap_or(0);

    if recursion_count > ZV_RECURSION_MAX {
        eprintln!(
            "Error: Too many recursive calls detected (depth: {}). \
             The zv binary may be calling itself infinitely.",
            recursion_count
        );
        std::process::exit(1);
    }
    Ok(())
}

mod app;
mod cli;
mod shell;
mod templates;
mod tools;
mod types;

pub use app::App;
pub use shell::*;
pub use templates::*;
pub use types::*;
