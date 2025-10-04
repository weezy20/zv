use super::ShellType;
use crate::tools::is_tty;
use sysinfo::System;

/// Get the parent process name using sysinfo
pub fn get_parent_process_name() -> Option<String> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::All,
        true,
        sysinfo::ProcessRefreshKind::everything(),
    );

    let current_pid = sysinfo::get_current_pid().ok()?;
    let current_process = system.process(current_pid)?;
    let parent_pid = current_process.parent()?;
    let parent_process = system.process(parent_pid)?;

    Some(parent_process.name().to_string_lossy().to_string())
}

/// Detect shell from parent process name
pub fn detect_shell_from_parent() -> Option<ShellType> {
    let parent_name = get_parent_process_name()?;
    detect_shell_from_string(&parent_name.to_lowercase())
}

/// Detect shell from any string containing shell information
fn detect_shell_from_string(shell_str: &str) -> Option<ShellType> {
    if shell_str.contains("bash") {
        Some(ShellType::Bash)
    } else if shell_str.contains("zsh") {
        Some(ShellType::Zsh)
    } else if shell_str.contains("fish") {
        Some(ShellType::Fish)
    } else if shell_str.contains("powershell") || shell_str.contains("pwsh") {
        Some(ShellType::PowerShell)
    } else if shell_str.contains("cmd") {
        Some(ShellType::Cmd)
    } else if shell_str.contains("tcsh") || shell_str.contains("csh") {
        Some(ShellType::Tcsh)
    } else if shell_str.contains("nu") {
        Some(ShellType::Nu)
    } else if shell_str.contains("sh") && !shell_str.contains("bash") && !shell_str.contains("zsh")
    {
        Some(ShellType::Posix)
    } else {
        None
    }
}

/// Main shell detection logic
pub fn detect_shell() -> ShellType {
    if cfg!(windows) {
        tracing::debug!(target: "shell_detection", "cfg!(windows)");
        detect_windows_shell()
    } else {
        tracing::debug!(target: "shell_detection", "cfg!(not(windows))");
        detect_unix_shell()
    }
}

/// Windows-specific shell detection
fn detect_windows_shell() -> ShellType {
    // First, try to detect from parent process if we're in a TTY
    if is_tty() {
        if let Some(shell) = detect_shell_from_parent() {
            return shell;
        }
    }

    // Check if we're in WSL (Unix shells on Windows)
    if std::env::var("WSL_DISTRO_NAME").is_ok() || std::env::var("WSL_INTEROP").is_ok() {
        // In WSL, SHELL variable should work properly
        if let Ok(shell) = std::env::var("SHELL") {
            if let Some(detected) = detect_shell_from_string(&shell) {
                return detected;
            }
        }
        return ShellType::Bash; // Default for WSL
    }

    // Check for PowerShell environment indicators
    if std::env::var("PSModulePath").is_ok() {
        return ShellType::PowerShell;
    }

    // Additional checks for specific environments
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        if term_program == "vscode" {
            // VS Code integrated terminal, check for shell preference
            if let Ok(vscode_shell) = std::env::var("VSCODE_SHELL_INTEGRATION") {
                if let Some(shell) = detect_shell_from_string(&vscode_shell) {
                    return shell;
                }
            }
        }
    }

    // Default to PowerShell on modern Windows
    ShellType::PowerShell
}

/// Unix-like systems shell detection
fn detect_unix_shell() -> ShellType {
    // First, try to detect from parent process if we're in a TTY
    if is_tty() {
        if let Some(shell) = detect_shell_from_parent() {
            return shell;
        }
    }

    // Use SHELL environment variable (standard on Unix-like systems)
    if let Ok(shell) = std::env::var("SHELL") {
        if let Some(detected) = detect_shell_from_string(&shell) {
            return detected;
        }
    }

    // Additional checks for specific environments
    if let Ok(term_program) = std::env::var("TERM_PROGRAM") {
        if term_program == "vscode" {
            // VS Code integrated terminal, check for shell preference
            if let Ok(vscode_shell) = std::env::var("VSCODE_SHELL_INTEGRATION") {
                if let Some(shell) = detect_shell_from_string(&vscode_shell) {
                    return shell;
                }
            }
        }
    }

    ShellType::Unknown
}
