use super::Shell;

/// Generate shell-specific environment content
pub fn generate_env_content(shell: &Shell, zv_dir: &str, zv_bin_path: &str) -> String {
    match shell {
        Shell::PowerShell => generate_powershell_content(zv_dir, zv_bin_path),
        Shell::Cmd => generate_cmd_content(zv_dir, zv_bin_path),
        Shell::Fish => generate_fish_content(zv_dir, zv_bin_path),
        Shell::Nu => generate_nu_content(zv_dir, zv_bin_path),
        Shell::Tcsh => generate_tcsh_content(zv_dir, zv_bin_path),
        Shell::Bash | Shell::Zsh | Shell::Posix | Shell::Unknown => {
            if matches!(shell, Shell::Unknown) {
                tracing::warn!("Unknown shell type detected, using POSIX shell syntax");
            }
            generate_posix_content(zv_dir, zv_bin_path)
        }
    }
}

/// Generate PowerShell environment setup script
pub fn generate_powershell_content(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"# zv shell setup for PowerShell
# To permanently set environment variables in PowerShell, run as Administrator:
# [Environment]::SetEnvironmentVariable("ZV_DIR", "{zv_dir}", "User")
# [Environment]::SetEnvironmentVariable("PATH", "{path};$env:PATH", "User")

$env:ZV_DIR = "{zv_dir}"
if ($env:PATH -notlike "*{path}*") {{
    $env:PATH = "{path};$env:PATH"
}}"#,
        path = zv_bin_path,
        zv_dir = zv_dir
    )
}

/// Generate Windows Command Prompt batch script
pub fn generate_cmd_content(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"REM zv shell setup for Command Prompt
REM To permanently set environment variables in CMD, run as Administrator:
REM setx ZV_DIR "{zv_dir}" /M
REM setx PATH "{path};%PATH%" /M

set "ZV_DIR={zv_dir}"
echo ;%PATH%; | find /i ";{path};" >nul || set "PATH={path};%PATH%""#,
        path = zv_bin_path,
        zv_dir = zv_dir
    )
}

/// Generate Fish shell setup script
pub fn generate_fish_content(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"#!/usr/bin/env fish
# zv shell setup for Fish shell
set -gx ZV_DIR "{zv_dir}"
if not contains "{path}" $PATH
    set -gx PATH "{path}" $PATH
end"#,
        path = zv_bin_path,
        zv_dir = zv_dir
    )
}

/// Generate Nushell setup script
pub fn generate_nu_content(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"# zv shell setup for Nushell
$env.ZV_DIR = "{zv_dir}"
$env.PATH = ($env.PATH | split row (char esep) | prepend "{path}" | uniq)"#,
        path = zv_bin_path,
        zv_dir = zv_dir
    )
}

/// Generate tcsh/csh setup script
pub fn generate_tcsh_content(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"#!/bin/csh
# zv shell setup for tcsh/csh
setenv ZV_DIR "{zv_dir}"
echo ":${{PATH}}:" | grep -q ":{path}:" || setenv PATH "{path}:$PATH""#,
        path = zv_bin_path,
        zv_dir = zv_dir
    )
}

/// Generate POSIX-compliant shell setup script (bash, zsh, sh)
pub fn generate_posix_content(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"#!/bin/sh
# zv shell setup
# affix colons on either side of $PATH to simplify matching
export ZV_DIR="{zv_dir}"
case ":${{PATH}}:" in
    *:"{path}":*)
        ;;
    *)
        # Prepending path in case a system-installed binary needs to be overridden
        export PATH="{path}:$PATH"
        ;;
esac"#,
        path = zv_bin_path,
        zv_dir = zv_dir
    )
}

/// Generate shell-specific uninstall/cleanup script
pub fn generate_cleanup_content(shell: &Shell, zv_dir: &str, zv_bin_path: &str) -> String {
    match shell {
        Shell::PowerShell => generate_powershell_cleanup(zv_dir, zv_bin_path),
        Shell::Cmd => generate_cmd_cleanup(zv_dir, zv_bin_path),
        Shell::Fish => generate_fish_cleanup(zv_dir, zv_bin_path),
        Shell::Nu => generate_nu_cleanup(zv_dir, zv_bin_path),
        Shell::Tcsh => generate_tcsh_cleanup(zv_dir, zv_bin_path),
        Shell::Bash | Shell::Zsh | Shell::Posix | Shell::Unknown => {
            generate_posix_cleanup(zv_dir, zv_bin_path)
        }
    }
}

/// Generate PowerShell cleanup script
fn generate_powershell_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"# zv cleanup script for PowerShell
# Remove zv from environment variables

Remove-Item Env:ZV_DIR -ErrorAction SilentlyContinue
$env:PATH = ($env:PATH -split ';' | Where-Object {{ $_ -ne "{path}" }}) -join ';'

Write-Host "zv environment cleaned up for current session"
Write-Host "To permanently remove, run as Administrator:"
Write-Host "[Environment]::SetEnvironmentVariable('ZV_DIR', `$null, 'User')"
Write-Host "Update PATH manually in System Properties -> Environment Variables""#,
        path = zv_bin_path
    )
}

/// Generate CMD cleanup script
fn generate_cmd_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"REM zv cleanup script for Command Prompt
set "ZV_DIR="
set "PATH=%PATH:{path};=%"
set "PATH=%PATH:;{path}=%"

echo zv environment cleaned up for current session
echo To permanently remove, run as Administrator:
echo setx ZV_DIR "" /M
echo Update PATH manually in System Properties"#,
        path = zv_bin_path
    )
}

/// Generate Fish cleanup script
fn generate_fish_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"#!/usr/bin/env fish
# zv cleanup script for Fish shell

set -e ZV_DIR
if set -l index (contains -i "{path}" $PATH)
    set -e PATH[$index]
end

echo "zv environment cleaned up""#,
        path = zv_bin_path
    )
}

/// Generate Nushell cleanup script  
fn generate_nu_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"# zv cleanup script for Nushell

hide-env ZV_DIR
$env.PATH = ($env.PATH | split row (char esep) | where $it != "{path}")

print "zv environment cleaned up""#,
        path = zv_bin_path
    )
}

/// Generate tcsh cleanup script
fn generate_tcsh_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"#!/bin/csh  
# zv cleanup script for tcsh/csh

unsetenv ZV_DIR
setenv PATH `echo $PATH | sed 's|{path}:||g' | sed 's|:{path}||g'`

echo "zv environment cleaned up""#,
        path = zv_bin_path
    )
}

/// Generate POSIX cleanup script
fn generate_posix_cleanup(zv_dir: &str, zv_bin_path: &str) -> String {
    format!(
        r#"#!/bin/sh
# zv cleanup script

unset ZV_DIR
case ":$PATH:" in
    *:"{path}":*)
        PATH=$(echo "$PATH" | sed -e "s|{path}:||g" -e "s|:{path}||g")
        export PATH
        ;;
esac

echo "zv environment cleaned up""#,
        path = zv_bin_path
    )
}

/// Generate shell-specific instructions for manual setup
pub fn generate_setup_instructions(shell: &Shell, env_file_path: &str) -> String {
    match shell {
        Shell::PowerShell => format!(
            r#"To setup zv for PowerShell:
1. For current session: . "{env_file_path}"
2. For permanent setup, add to your PowerShell profile:
   - Run: notepad $PROFILE
   - Add: . "{env_file_path}"
   - Or run as Administrator for system-wide:
     [Environment]::SetEnvironmentVariable("ZV_DIR", "your_path", "Machine")"#
        ),
        Shell::Cmd => format!(
            r#"To setup zv for Command Prompt:
1. For current session: call "{env_file_path}"
2. For permanent setup, run as Administrator:
   setx PATH "%PATH%;your_bin_path" /M"#
        ),
        Shell::Fish => format!(
            r#"To setup zv for Fish shell:
1. For current session: source "{env_file_path}"
2. For permanent setup, add to ~/.config/fish/config.fish:
   source "{env_file_path}""#
        ),
        Shell::Nu => format!(
            r#"To setup zv for Nushell:
1. For current session: source "{env_file_path}"  
2. For permanent setup, add to ~/.config/nushell/config.nu:
   source "{env_file_path}""#
        ),
        Shell::Bash => format!(
            r#"To setup zv for Bash:
1. For current session: source "{env_file_path}"
2. For permanent setup, add to ~/.bashrc or ~/.profile:
   source "{env_file_path}""#
        ),
        Shell::Zsh => format!(
            r#"To setup zv for Zsh:
1. For current session: source "{env_file_path}"
2. For permanent setup, add to ~/.zshrc or ~/.zprofile:
   source "{env_file_path}""#
        ),
        Shell::Tcsh => format!(
            r#"To setup zv for tcsh/csh:
1. For current session: source "{env_file_path}"
2. For permanent setup, add to ~/.tcshrc or ~/.cshrc:
   source "{env_file_path}""#
        ),
        Shell::Posix | Shell::Unknown => format!(
            r#"To setup zv for your shell:
1. For current session: source "{env_file_path}"
2. For permanent setup, add to your shell's profile file:
   source "{env_file_path}""#
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_powershell_content() {
        let content = generate_powershell_content("C:\\zv", "C:\\zv\\bin");
        assert!(content.contains("$env:ZV_DIR"));
        assert!(content.contains("$env:PATH"));
        assert!(content.contains("C:\\zv"));
        assert!(content.contains("C:\\zv\\bin"));
    }

    #[test]
    fn test_generate_fish_content() {
        let content = generate_fish_content("/home/user/.zv", "/home/user/.zv/bin");
        assert!(content.contains("set -gx ZV_DIR"));
        assert!(content.contains("set -gx PATH"));
        assert!(content.contains("/home/user/.zv"));
    }

    #[test]
    fn test_generate_posix_content() {
        let content = generate_posix_content("/home/user/.zv", "/home/user/.zv/bin");
        assert!(content.contains("export ZV_DIR"));
        assert!(content.contains("export PATH"));
        assert!(content.contains("case"));
        assert!(content.contains("/home/user/.zv"));
    }

    #[test]
    fn test_generate_cleanup_content() {
        let cleanup = generate_cleanup_content(&Shell::Fish, "/home/user/.zv", "/home/user/.zv/bin");
        assert!(cleanup.contains("set -e ZV_DIR"));
        assert!(cleanup.contains("set -e PATH"));
    }

    #[test]
    fn test_generate_setup_instructions() {
        let instructions = generate_setup_instructions(&Shell::Bash, "/home/user/.zv/env");
        assert!(instructions.contains("source"));
        assert!(instructions.contains("~/.bashrc"));
        assert!(instructions.contains("/home/user/.zv/env"));
    }
}