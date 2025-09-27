use crate::{ArchiveExt, Shim};

#[derive(Debug, Default)]
pub struct ToolchainManager {
    versions_path: std::path::PathBuf,
}
impl ToolchainManager {
    pub fn new(versions_path: impl AsRef<std::path::Path>) -> Self {
        Self {
            versions_path: versions_path.as_ref().to_path_buf(),
        }
    }
    /// Basic checks to see is a zig <version> (optionally nested) is installed
    pub fn is_version_installed(&self, version: &str, nested: Option<&str>) -> bool {
        let version_path = if let Some(n) = nested {
            self.versions_path.join(n).join(version)
        } else {
            self.versions_path.join(version)
        };
        if !(version_path.exists() && version_path.is_dir()) {
            return false;
        }
        let zig_exe = Shim::Zig.executable_name();
        if version_path.join(zig_exe).is_file() {
            return true;
        }
        false
    }
    /// Installs a zig version from a tarball / zipball path
    pub async fn install_version(
        &self,
        archive_path: &std::path::Path,
        version: &semver::Version,
        nested: Option<&str>,
        ext: ArchiveExt,
    ) -> crate::Result<()> {
        let extract_path = if let Some(n) = nested {
            self.versions_path.join(n).join(version.to_string())
        } else {
            self.versions_path.join(version.to_string())
        };
        tokio::fs::create_dir_all(&extract_path).await?;

        let bytes = tokio::fs::read(archive_path).await?;

        match ext {
            ArchiveExt::TarXz => {
                use tar::Archive;
                let xz = xz2::read::XzDecoder::new(std::io::Cursor::new(bytes));
                let mut archive = Archive::new(xz);
                archive.unpack(&extract_path)?;
            }
            ArchiveExt::Zip => {
                use std::io::Write;
                let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i)?;
                    let out_path = extract_path.join(file.name());

                    if file.is_dir() {
                        tokio::fs::create_dir_all(&out_path).await?;
                    } else {
                        if let Some(p) = out_path.parent() {
                            tokio::fs::create_dir_all(p).await?;
                        }
                        let mut out = std::fs::File::create(&out_path)?;
                        std::io::copy(&mut file, &mut out)?;
                    }
                }
            }
        }

        Ok(())
    }
}
