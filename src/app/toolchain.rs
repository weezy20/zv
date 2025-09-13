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
}
