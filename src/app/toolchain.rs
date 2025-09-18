#[derive(Debug, Default)]
pub struct ToolchainManager {
    path: std::path::PathBuf,
}
impl ToolchainManager {
    pub fn new(path: impl AsRef<std::path::Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
    pub fn is_version_installed(&self, version: &str, nested: Option<&str>) -> bool {
        let version_path = if let Some(n) = nested {
            self.path.join(n).join(version)
        } else {
            self.path.join(version)
        };
        version_path.exists() && version_path.is_dir()
    }
}
