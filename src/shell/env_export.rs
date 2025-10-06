use super::{Shell, path_utils::*};
use crate::{ZvError, app::App};
use color_eyre::eyre::eyre;
use std::path::Path;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

/// Write shell file content with proper line endings for cross-platform compatibility
pub async fn write_shell_file_with_line_endings(
    file_path: &Path,
    content: &str,
) -> Result<(), ZvError> {
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(file_path)
        .await
        .map_err(|e| {
            ZvError::ZvExportError(eyre!(e).wrap_err(format!(
                "Failed to open file for writing: {}",
                file_path.display()
            )))
        })?;

    // Normalize line endings based on file type
    let normalized_content = normalize_line_endings_for_file(file_path, content);

    file.write_all(normalized_content.as_bytes())
        .await
        .map_err(|e| ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write to file")))?;

    // Add final newline with appropriate line ending
    let final_newline = if should_use_unix_line_endings(file_path) {
        "\n"
    } else {
        "\r\n"
    };

    file.write_all(final_newline.as_bytes())
        .await
        .map_err(|e| {
            ZvError::ZvExportError(eyre!(e).wrap_err("Failed to write newline to file"))
        })?;

    Ok(())
}

impl Shell {
    /// Returns the env file path and content without writing to disk
    pub fn export_without_dump<'a>(&self, app: &'a App, using_env_var: bool) -> (&'a Path, String) {
        let (zv_dir_str, zv_bin_path_str) = get_path_strings(self, app, using_env_var);
        let env_content = self.generate_env_content(&zv_dir_str, &zv_bin_path_str, using_env_var);

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
            Ok(existing_content) => {
                // Normalize both contents for comparison to handle line ending differences
                let normalized_existing = normalize_line_endings_for_comparison(&existing_content);
                let normalized_new = normalize_line_endings_for_comparison(content);
                normalized_existing.trim() != normalized_new.trim()
            }
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

/// Normalize line endings for content comparison (convert all to LF)
fn normalize_line_endings_for_comparison(content: &str) -> String {
    content.replace("\r\n", "\n")
}

/// Write content to environment file with proper line endings
async fn write_env_file(env_file: &Path, content: &str) -> Result<(), ZvError> {
    write_shell_file_with_line_endings(env_file, content).await
}

/// Normalize line endings based on the target file type
fn normalize_line_endings_for_file(env_file: &Path, content: &str) -> String {
    if should_use_unix_line_endings(env_file) {
        // Convert any CRLF to LF for Unix-style files
        content.replace("\r\n", "\n")
    } else {
        // Convert LF to CRLF for Windows-style files, but avoid double conversion
        content.replace("\r\n", "\n").replace('\n', "\r\n")
    }
}

/// Determine if a file should use Unix line endings (LF) based on its extension
fn should_use_unix_line_endings(env_file: &Path) -> bool {
    match env_file.extension().and_then(|ext| ext.to_str()) {
        // Windows-specific file types should use CRLF
        Some("bat") | Some("cmd") | Some("ps1") => false,
        // All other shell files (including no extension) should use LF
        // This includes: .sh, .fish, .nu, .csh files and the plain "env" file
        _ => true,
    }
}
