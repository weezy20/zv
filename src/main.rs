#![allow(unused, warnings)]
use color_eyre::{
    Result,
    config::{HookBuilder, Theme},
    eyre::Context,
};
use tracing_subscriber::prelude::*;

// We only expect to route to `zig` or `zls` once from `zv`
// For example: `zv init --zig`  => `zv` spawns `zig`, +1 in [instantiate_zig]
const ZV_RECURSION_MAX: u32 = 1;

#[tokio::main]
async fn main() -> Result<()> {
    // Apply security mitigations as early as possible
    #[cfg(windows)]
    apply_windows_security_mitigations();

    check_recursion_with_context("zv main")?;

    #[cfg(feature = "dotenv")]
    dotenv::dotenv().ok();

    // Initialize color support
    yansi::whenever(yansi::Condition::TTY_AND_COLOR);

    // Set up error reporting with color-aware themes
    if yansi::is_enabled() {
        HookBuilder::default()
            .display_env_section(cfg!(debug_assertions))
            .display_location_section(cfg!(debug_assertions))
            .install()?;
    } else {
        HookBuilder::default()
            .theme(Theme::new())
            .display_env_section(cfg!(debug_assertions))
            .display_location_section(cfg!(debug_assertions))
            .install()?;
    }

    // Set up tracing with progress bar support
    init_tracing()?;

    let program_name = get_program_name()?;

    match program_name.as_str() {
        "zv" => cli::zv_main().await,
        "zig" => cli::zig_main().await,
        "zls" => cli::zls_main().await,
        _ => {
            eprintln!(
                "Unknown invocation: {}. This binary should be invoked as 'zv', 'zig', or 'zls'.",
                program_name
            );
            std::process::exit(1);
        }
    }
}

/// Initialize tracing with dual-mode logging
///
/// - If ZV_LOG is not set: Simple "info: message" format for user-friendly output  
/// - If ZV_LOG is set: Full structured tracing with timestamps and module paths
fn init_tracing() -> Result<()> {
    let zv_log = std::env::var("ZV_LOG").is_ok();

    if zv_log {
        // Full structured logging mode
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true) // Show module paths
                    .with_filter(
                        tracing_subscriber::EnvFilter::try_from_env("ZV_LOG")
                            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("zv=warn")),
                    ),
            )
            .init();
    } else {
        // Simple user-friendly logging mode
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false) // Hide module paths
                    .with_level(true) // Show level
                    .with_thread_ids(false)
                    .with_thread_names(false)
                    .with_file(false)
                    .with_line_number(false)
                    .without_time() // No timestamps
                    .with_filter(tracing_subscriber::EnvFilter::new("zv=info")),
            )
            .init();
    }

    Ok(())
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

/// Apply Windows-specific security mitigations to prevent DLL hijacking
///
/// This function should be called as early as possible in main(), before any
/// dynamic library loading occurs. It restricts DLL loading to trusted system
/// directories only, preventing malicious DLLs from being loaded from the
/// current directory or arbitrary PATH locations.
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

    tracing::debug!("Applied Windows DLL security mitigations");
}

/// Check recursion depth with context for better error messages
pub fn check_recursion_with_context(context: &str) -> Result<()> {
    // Recursion guard - prevent infinite loops but allow zig subcommands such as zv init --zig :  zv -> zig
    let recursion_count = std::env::var("ZV_RECURSION_COUNT")
        .unwrap_or_else(|_| "0".to_string())
        .parse::<u32>()
        .unwrap_or(0);

    if recursion_count > ZV_RECURSION_MAX {
        eprintln!(
            "Error: Too many recursive calls detected in {} (depth: {}). \
             The zv binary may be calling itself infinitely.",
            context, recursion_count
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
