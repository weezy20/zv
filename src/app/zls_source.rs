use crate::{Shim, ZvError};
use color_eyre::eyre::eyre;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const ZLS_GIT_URL: &str = "https://github.com/zigtools/zls";

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

/// Ensure the shared ZLS git checkout exists at `cache_src` and is up to date.
/// Performs a one-time full clone, then `git fetch` on subsequent calls.
async fn ensure_zls_clone(cache_src: &Path) -> Result<(), ZvError> {
    if let Some(parent) = cache_src.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(ZvError::Io)?;
    }

    if cache_src.join(".git").is_dir() {
        run_git(&["fetch", "--tags", "--prune", "origin"], Some(cache_src))?;
    } else {
        if cache_src.exists() {
            tokio::fs::remove_dir_all(cache_src)
                .await
                .map_err(ZvError::Io)?;
        }
        run_git(
            &["clone", ZLS_GIT_URL, cache_src.to_string_lossy().as_ref()],
            None,
        )?;
    }
    Ok(())
}

/// Reset the shared checkout to a clean state on the requested ref so every
/// build starts from a working tree that exactly matches upstream — no user
/// mutations, no leftover artifacts from a prior build. Gitignored paths
/// (`.zig-cache/`, `zig-out/`) are preserved so repeat builds stay fast.
fn checkout_ref(cache_src: &Path, zls_version: &str) -> Result<(), ZvError> {
    let checkout_ref = resolve_checkout_ref(zls_version)?;

    let _ = run_git(&["reset", "--hard", "HEAD"], Some(cache_src));

    let checkout_result = run_git(&["checkout", &checkout_ref], Some(cache_src));
    if checkout_result.is_err() && extract_commit_hash(zls_version).is_some() {
        // Specific commit not in our refs (rare — force-push or detached). Pull it directly.
        run_git(&["fetch", "origin", &checkout_ref], Some(cache_src))?;
        run_git(&["checkout", &checkout_ref], Some(cache_src))?;
    } else {
        checkout_result?;
    }

    run_git(&["clean", "-fd"], Some(cache_src))?;

    Ok(())
}

pub async fn build_zls_from_source(
    zls_version: &str,
    active_zig_exe: &Path,
    cache_src: &Path,
    dest_dir: &Path,
) -> Result<PathBuf, ZvError> {
    if !dest_dir.exists() {
        tokio::fs::create_dir_all(dest_dir)
            .await
            .map_err(ZvError::Io)?;
    }

    ensure_zls_clone(cache_src).await?;
    checkout_ref(cache_src, zls_version)?;

    let recursion_count = std::env::var("ZV_RECURSION_COUNT")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let status = Command::new(active_zig_exe)
        .arg("build")
        .arg("-Doptimize=ReleaseSafe")
        .current_dir(cache_src)
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

    let built_binary = cache_src
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
