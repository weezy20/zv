use crate::{Shim, ZvError};
use color_eyre::eyre::eyre;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn run_git(args: &[&str], cwd: Option<&Path>) -> Result<(), ZvError> {
    let mut cmd = Command::new("git");
    cmd.args(args);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    let status = cmd
        .status()
        .map_err(|e| ZvError::General(eyre!("Failed to execute git: {e}")))?;

    if status.success() {
        Ok(())
    } else {
        Err(ZvError::General(eyre!(
            "git command failed: git {}",
            args.join(" ")
        )))
    }
}

pub fn extract_commit_hash(version: &str) -> Option<&str> {
    version.split_once('+').map(|(_, suffix)| suffix)
}

fn resolve_checkout_ref(zls_version: &str) -> Result<String, ZvError> {
    if zls_version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Ok(zls_version.to_string());
    }

    if let Some(hash) = extract_commit_hash(zls_version)
        && !hash.is_empty()
    {
        return Ok(hash.to_string());
    }

    Err(ZvError::General(eyre!(
        "Unexpected ZLS version format '{}'. Try `zv zls --download`.",
        zls_version
    )))
}

pub async fn build_zls_from_source(
    zls_version: &str,
    active_zig_exe: &Path,
    dest_dir: &Path,
) -> Result<PathBuf, ZvError> {
    let source_dir = dest_dir.join(".source");
    if source_dir.exists() {
        tokio::fs::remove_dir_all(&source_dir)
            .await
            .map_err(ZvError::Io)?;
    }

    if !dest_dir.exists() {
        tokio::fs::create_dir_all(dest_dir)
            .await
            .map_err(ZvError::Io)?;
    }

    run_git(
        &[
            "clone",
            "--depth",
            "50",
            "https://github.com/zigtools/zls",
            source_dir.to_string_lossy().as_ref(),
        ],
        None,
    )?;

    let checkout_ref = resolve_checkout_ref(zls_version)?;
    let checkout_result = run_git(&["checkout", &checkout_ref], Some(&source_dir));
    if checkout_result.is_err() && extract_commit_hash(zls_version).is_some() {
        run_git(&["fetch", "--unshallow"], Some(&source_dir))?;
        run_git(&["checkout", &checkout_ref], Some(&source_dir))?;
    } else {
        checkout_result?;
    }

    let recursion_count = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let status = Command::new(active_zig_exe)
        .arg("build")
        .arg("-Doptimize=ReleaseSafe")
        .current_dir(&source_dir)
        .env("ZV_RECURSION_COUNT", (recursion_count + 1).to_string())
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| ZvError::General(eyre!("Failed to run Zig build for ZLS: {e}")))?;

    if !status.success() {
        return Err(ZvError::General(eyre!(
            "Failed to build ZLS from source with '{}'. Try `zv zls --download`.",
            active_zig_exe.display()
        )));
    }

    let built_binary = source_dir
        .join("zig-out")
        .join("bin")
        .join(Shim::Zls.executable_name());
    if !built_binary.is_file() {
        return Err(ZvError::General(eyre!(
            "ZLS build finished but binary was not found at {}",
            built_binary.display()
        )));
    }

    let output_binary = dest_dir.join(Shim::Zls.executable_name());
    tokio::fs::copy(&built_binary, &output_binary)
        .await
        .map_err(ZvError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = tokio::fs::metadata(&output_binary)
            .await
            .map_err(ZvError::Io)?
            .permissions();
        permissions.set_mode(0o755);
        tokio::fs::set_permissions(&output_binary, permissions)
            .await
            .map_err(ZvError::Io)?;
    }

    Ok(output_binary)
}

#[cfg(test)]
mod tests {
    use super::extract_commit_hash;

    #[test]
    fn extracts_commit_hash_from_nightly_version() {
        let version = "0.17.0-dev.10+1ebcc794";
        assert_eq!(extract_commit_hash(version), Some("1ebcc794"));
    }
}
