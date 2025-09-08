use super::{Shell, generators::*, path_utils::*};
use crate::{ZvError, app::App};
use color_eyre::eyre::eyre;
use std::path::Path;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

impl Shell {
    /// Returns the env file path and content without writing to disk
    pub fn export_without_dump<'a>(&self, app: &'a App, using_env_var: bool) -> (&'a Path, String) {
        let (zv_dir_str, zv_bin_path_str) = get_path_strings(self, app, using_env_var);
        let env_content = self.generate_env_content(&zv_dir_str, &zv_bin_path_str);

        (app.env_path().as_path(), env_content)
    }

    /// Dumps shell specific environment variables to the env file - Skips for windows shell
    pub async fn export_unix(&self, app: &App, using_env_var: bool) -> Result<(), ZvError> {
        // Skip file operations for Windows shells that use direct system variable edits
        // But allow PowerShell on Unix to create env files
        if self.windows_shell() && !self.is_powershell_in_unix() {
            return Ok(());
        }

        let (env_file, content) = self.export_without_dump(app, using_env_var);
        write_env_file_if_needed(env_file, &content).await
    }
    /// Check if shell uses direct system variable edits (no file writing needed)
    #[inline]
    fn windows_shell(&self) -> bool {
        use super::ShellType;
        matches!(self.shell_type, ShellType::Cmd | ShellType::PowerShell)
    }
}

/// Write environment file only if content is different or file doesn't exist
async fn write_env_file_if_needed(env_file: &Path, content: &str) -> Result<(), ZvError> {
    let should_write = if env_file.exists() {
        match tokio::fs::read_to_string(env_file).await {
            Ok(existing_content) => existing_content.trim() != content.trim(),
            Err(_) => {
                tracing::warn!("Could not read existing env file, will overwrite");
                true
            }
        }
    } else {
        true
    };

    if should_write {
        write_env_file(env_file, content).await?;
    }

    Ok(())
}

/// Write content to environment file
async fn write_env_file(env_file: &Path, content: &str) -> Result<(), ZvError> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(env_file)
        .await
        .map_err(|e| {
            ZvError::ZvExportError(eyre!(e).wrap_err(format!(
                "Failed to open env file for writing: {}",
                env_file.display()
            )))
        })?;

    file.write_all(content.as_bytes())
        .await
        .map_err(|e| ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write to env file")))?;

    file.write_all(b"\n").await.map_err(|e| {
        ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write newline to env file"))
    })?;

    Ok(())
}
