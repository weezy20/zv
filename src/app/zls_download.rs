use crate::app::constants::ZLS_MINISIGN_PUBKEY;
use crate::app::network::zls::ZlsRelease;
use crate::app::utils::remove_files;
use crate::{App, ArchiveExt, Shim, ZvError};
use color_eyre::eyre::eyre;
use std::path::{Path, PathBuf};

fn archive_extension_from_name(name: &str) -> Result<ArchiveExt, ZvError> {
    if name.ends_with(".zip") {
        Ok(ArchiveExt::Zip)
    } else if name.ends_with(".tar.xz") {
        Ok(ArchiveExt::TarXz)
    } else {
        Err(ZvError::General(eyre!(
            "Unsupported ZLS artifact extension for '{}'",
            name
        )))
    }
}

fn extract_filename_from_url(url: &str) -> Result<String, ZvError> {
    url.split('/')
        .next_back()
        .filter(|name| !name.is_empty())
        .map(std::string::ToString::to_string)
        .ok_or_else(|| ZvError::General(eyre!("Could not derive filename from URL '{}'.", url)))
}

async fn extract_zls_binary(
    archive_path: &Path,
    ext: ArchiveExt,
    dest_dir: &Path,
) -> Result<PathBuf, ZvError> {
    let temp_dir = dest_dir.join(".extract-tmp");
    if temp_dir.exists() {
        tokio::fs::remove_dir_all(&temp_dir)
            .await
            .map_err(ZvError::Io)?;
    }
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(ZvError::Io)?;

    let bytes = tokio::fs::read(archive_path).await.map_err(ZvError::Io)?;
    match ext {
        ArchiveExt::TarXz => {
            let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(bytes));
            let mut archive = tar::Archive::new(xz);
            archive.unpack(&temp_dir).map_err(|e| {
                ZvError::General(eyre!("Failed to extract ZLS tar.xz archive: {e}"))
            })?;
        }
        ArchiveExt::Zip => {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
                .map_err(|e| ZvError::General(eyre!("Failed to open ZLS zip archive: {e}")))?;
            for idx in 0..archive.len() {
                let mut file = archive
                    .by_index(idx)
                    .map_err(|e| ZvError::General(eyre!("Failed to read ZLS zip entry: {e}")))?;
                let output = temp_dir.join(file.enclosed_name().ok_or_else(|| {
                    ZvError::General(eyre!(
                        "Refusing to extract unsafe ZLS zip entry '{}'",
                        file.name()
                    ))
                })?);
                if file.is_dir() {
                    tokio::fs::create_dir_all(&output)
                        .await
                        .map_err(ZvError::Io)?;
                } else {
                    if let Some(parent) = output.parent() {
                        tokio::fs::create_dir_all(parent)
                            .await
                            .map_err(ZvError::Io)?;
                    }
                    let mut out_file = std::fs::File::create(&output).map_err(ZvError::Io)?;
                    std::io::copy(&mut file, &mut out_file).map_err(ZvError::Io)?;
                }
            }
        }
    }

    let binary_name = Shim::Zls.executable_name();
    let mut source_binary: Option<PathBuf> = None;
    for entry in walkdir::WalkDir::new(&temp_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if entry.file_name() == std::ffi::OsStr::new(binary_name) {
            source_binary = Some(entry.path().to_path_buf());
            break;
        }
    }

    let source_binary = source_binary.ok_or_else(|| {
        ZvError::General(eyre!(
            "Unable to locate '{}' in downloaded ZLS archive",
            binary_name
        ))
    })?;

    let destination_binary = dest_dir.join(binary_name);
    tokio::fs::copy(&source_binary, &destination_binary)
        .await
        .map_err(ZvError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = tokio::fs::metadata(&destination_binary)
            .await
            .map_err(ZvError::Io)?
            .permissions();
        permissions.set_mode(0o755);
        tokio::fs::set_permissions(&destination_binary, permissions)
            .await
            .map_err(ZvError::Io)?;
    }

    tokio::fs::remove_dir_all(&temp_dir)
        .await
        .map_err(ZvError::Io)?;

    Ok(destination_binary)
}

pub async fn download_zls_prebuilt(
    app: &mut App,
    release: &ZlsRelease,
    host_target: &str,
    dest_dir: &Path,
) -> Result<PathBuf, ZvError> {
    let artifact = release.artifact_for_target(host_target).ok_or_else(|| {
        ZvError::General(eyre!(
            "No ZLS artifact available for host target '{}' in release {}",
            host_target,
            release.version
        ))
    })?;

    let archive_name = extract_filename_from_url(&artifact.tarball)?;
    let archive_ext = archive_extension_from_name(&archive_name)?;
    let minisig_url = format!("{}.minisig", artifact.tarball);

    app.ensure_network().await?;
    let download = app
        .network
        .as_ref()
        .ok_or_else(|| ZvError::General(eyre!("Network client is not initialized")))?
        .direct_download(
            &artifact.tarball,
            &minisig_url,
            &archive_name,
            ZLS_MINISIGN_PUBKEY,
            Some(&artifact.shasum),
            Some(artifact.size),
        )
        .await?;

    if !dest_dir.exists() {
        tokio::fs::create_dir_all(dest_dir)
            .await
            .map_err(ZvError::Io)?;
    }

    let binary_path = extract_zls_binary(&download.tarball_path, archive_ext, dest_dir).await?;
    remove_files(&[download.tarball_path, download.minisig_path]).await;
    Ok(binary_path)
}

#[cfg(test)]
mod tests {
    use super::extract_zls_binary;
    use crate::{ArchiveExt, Shim};
    use std::io::Write;
    use std::path::Path;
    use zip::write::SimpleFileOptions;

    fn write_zip_entry(archive_path: &Path, name: &str, contents: &[u8]) {
        let file = std::fs::File::create(archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(contents).unwrap();
        zip.finish().unwrap();
    }

    #[tokio::test]
    async fn rejects_unsafe_zip_entry_paths() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("zls.zip");
        write_zip_entry(&archive_path, "../../../zls", b"bad");

        let dest_dir = temp.path().join("dest");
        let err = extract_zls_binary(&archive_path, ArchiveExt::Zip, &dest_dir)
            .await
            .unwrap_err();

        assert!(err.to_string().contains("unsafe ZLS zip entry"));
        assert!(!temp.path().join("zls").exists());
    }

    #[tokio::test]
    async fn extracts_safe_zip_entry_paths() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("zls.zip");
        let binary_name = Shim::Zls.executable_name();
        write_zip_entry(
            &archive_path,
            &format!("zls/bin/{binary_name}"),
            b"zls-binary",
        );

        let dest_dir = temp.path().join("dest");
        let binary_path = extract_zls_binary(&archive_path, ArchiveExt::Zip, &dest_dir)
            .await
            .unwrap();

        assert_eq!(binary_path, dest_dir.join(binary_name));
        assert_eq!(std::fs::read(binary_path).unwrap(), b"zls-binary");
    }
}
