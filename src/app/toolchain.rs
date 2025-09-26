use crate::Shim;

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
}
