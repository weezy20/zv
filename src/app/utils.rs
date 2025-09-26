use crate::{
    ZigVersion, ZvError,
    tools::canonicalize,
    types::{ArchiveExt, Shim},
};
use color_eyre::eyre::eyre;
use indicatif::{ProgressBar, ProgressStyle};
use same_file::Handle;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Checks if a file is a valid zv shim by comparing it with the current executable
fn is_zv_shim(shim_path: &Path, current_exe_handle: &Handle) -> bool {
    // First check for hard links using same-file crate
    if let Ok(shim_handle) = Handle::from_path(shim_path) {
        if shim_handle == *current_exe_handle {
            tracing::debug!("Found ZV shim (hard link) at {:?}", shim_path);
            return true;
        }
    }

    // Check for symlinks
    if shim_path.is_symlink() {
        if let Ok(target) = std::fs::read_link(shim_path) {
            // Handle both absolute and relative symlink targets
            let resolved_target = if target.is_absolute() {
                canonicalize(&target)
            } else {
                // For relative symlinks, resolve relative to the symlink's parent directory
                if let Some(parent) = shim_path.parent() {
                    canonicalize(parent.join(&target))
                } else {
                    canonicalize(&target)
                }
            };

            if let Ok(resolved_target) = resolved_target {
                // Compare the resolved target with current exe using same-file
                if let Ok(target_handle) = Handle::from_path(&resolved_target) {
                    if target_handle == *current_exe_handle {
                        tracing::debug!("Found ZV shim (symlink) at {:?}", shim_path);
                        return true;
                    }
                }
            }
        }
    }

    false
}

/// Detect and validate ZV shim in the bin directory
/// Returns the canonicalized path if a valid ZV shim is found
pub fn detect_shim(bin_path: &Path, shim: Shim) -> Option<PathBuf> {
    let shim_file = bin_path.join(shim.executable_name());

    // Basic existence and file type check
    if !shim_file.is_file() {
        return None;
    }

    // Get current executable handle for comparison
    let current_exe_path = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            tracing::warn!("Failed to get current executable path: {}", e);
            return None;
        }
    };

    let current_exe_handle = match Handle::from_path(&current_exe_path) {
        Ok(handle) => handle,
        Err(e) => {
            tracing::warn!("Failed to create handle for current executable: {}", e);
            return None;
        }
    };

    #[cfg(unix)]
    {
        // On Unix, also check if the file is executable
        if let Ok(metadata) = std::fs::metadata(&shim_file) {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                tracing::debug!(
                    "File {} exists but is not executable",
                    shim.executable_name()
                );
                return None;
            }
        }
    }

    // Check if this is actually a zv shim
    if is_zv_shim(&shim_file, &current_exe_handle) {
        canonicalize(&shim_file).ok()
    } else {
        tracing::debug!(
            "File {} exists but is not a zv shim at {:?}",
            shim.executable_name(),
            shim_file
        );
        None
    }
}

/// Construct the zig tarball name based on HOST arch, os. zig 0.14.1 onwards, the naming convention changed
/// to {arch}-{os}-{version}
pub fn zig_tarball(
    semver_version: &semver::Version,
    extension: Option<ArchiveExt>,
) -> Option<String> {
    use target_lexicon::HOST;
    let arch = match HOST.architecture {
        target_lexicon::Architecture::X86_64 => "x86_64",
        target_lexicon::Architecture::Aarch64(_) => "aarch64",
        target_lexicon::Architecture::X86_32(_) => "x86",
        target_lexicon::Architecture::Arm(_) => "arm",
        target_lexicon::Architecture::Riscv64(_) => "riscv64",
        target_lexicon::Architecture::Powerpc64 => "powerpc64",
        target_lexicon::Architecture::Powerpc64le => "powerpc64le",
        target_lexicon::Architecture::S390x => "s390x",
        target_lexicon::Architecture::LoongArch64 => "loongarch64",
        _ => return None,
    };

    let os = match HOST.operating_system {
        target_lexicon::OperatingSystem::Linux => "linux",
        target_lexicon::OperatingSystem::Darwin(_) => "macos",
        target_lexicon::OperatingSystem::Windows => "windows",
        target_lexicon::OperatingSystem::Freebsd => "freebsd",
        target_lexicon::OperatingSystem::Netbsd => "netbsd",
        _ => return None,
    };
    let ext = if let Some(ext) = extension {
        ext
    } else if HOST.operating_system == target_lexicon::OperatingSystem::Windows {
        ArchiveExt::Zip
    } else {
        ArchiveExt::TarXz
    };
    if semver_version.le(&semver::Version::new(0, 14, 0)) {
        return Some(format!("zig-{os}-{arch}-{semver_version}.{ext}"));
    } else {
        return Some(format!("zig-{arch}-{os}-{semver_version}.{ext}"));
    }
}

/// Returns the host target string in the format used by Zig releases
pub fn host_target() -> Option<String> {
    use target_lexicon::HOST;

    let arch = match HOST.architecture {
        target_lexicon::Architecture::X86_64 => "x86_64",
        target_lexicon::Architecture::Aarch64(_) => "aarch64",
        target_lexicon::Architecture::X86_32(_) => "x86",
        target_lexicon::Architecture::Arm(_) => "arm",
        target_lexicon::Architecture::Riscv64(_) => "riscv64",
        target_lexicon::Architecture::Powerpc64 => "powerpc64",
        target_lexicon::Architecture::Powerpc64le => "powerpc64le",
        target_lexicon::Architecture::S390x => "s390x",
        target_lexicon::Architecture::LoongArch64 => "loongarch64",
        _ => return None,
    };

    let os = match HOST.operating_system {
        target_lexicon::OperatingSystem::Linux => "linux",
        target_lexicon::OperatingSystem::Darwin(_) => "macos",
        target_lexicon::OperatingSystem::Windows => "windows",
        target_lexicon::OperatingSystem::Freebsd => "freebsd",
        target_lexicon::OperatingSystem::Netbsd => "netbsd",
        _ => return None,
    };

    Some(format!("{arch}-{os}"))
}

/// User-Agent string for network requests
pub const fn zv_agent() -> &'static str {
    concat!("zv-cli/", env!("CARGO_PKG_VERSION"))
}

/// Messages that can be sent to the progress bar actor
#[derive(Debug, Clone)]
pub enum ProgressMessage {
    Start { message: String },
    Update { message: String },
    Finish { message: String },
    FinishWithError { message: String },
    Shutdown,
}

/// Progress bar actor that runs in its own thread
struct ProgressActor {
    rx: tokio::sync::mpsc::Receiver<ProgressMessage>,
}

impl ProgressActor {
    fn run(mut self) {
        let mut spinner: Option<ProgressBar> = None;

        while let Some(msg) = self.rx.blocking_recv() {
            match msg {
                ProgressMessage::Start { message } => {
                    let pb = ProgressBar::new_spinner();
                    pb.set_style(
                        ProgressStyle::default_spinner()
                            .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
                            .template("{spinner:.blue} {msg}")
                            .unwrap(),
                    );
                    pb.set_message(message);
                    pb.enable_steady_tick(Duration::from_millis(120));
                    spinner = Some(pb);
                }
                ProgressMessage::Update { message } => {
                    if let Some(ref pb) = spinner {
                        pb.set_message(message);
                    }
                }
                ProgressMessage::Finish { message } => {
                    if let Some(pb) = spinner.take() {
                        pb.finish_with_message(message);
                    }
                }
                ProgressMessage::FinishWithError { message } => {
                    if let Some(pb) = spinner.take() {
                        pb.finish_with_message(message);
                    }
                }
                ProgressMessage::Shutdown => {
                    if let Some(pb) = spinner.take() {
                        pb.finish_and_clear();
                    }
                    break;
                }
            }
        }
    }
}

/// Handle to a progress bar actor with automatic cleanup
pub struct ProgressHandle {
    tx: tokio::sync::mpsc::Sender<ProgressMessage>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl ProgressHandle {
    /// Spawn a new progress bar actor in its own thread
    pub fn spawn() -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        let handle = std::thread::spawn(move || {
            let actor = ProgressActor { rx };
            actor.run();
        });

        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Send a message to the progress bar actor
    pub async fn send(
        &self,
        msg: ProgressMessage,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<ProgressMessage>> {
        self.tx.send(msg).await
    }

    /// Start the progress bar with a message
    pub async fn start(
        &self,
        message: impl Into<String>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<ProgressMessage>> {
        self.send(ProgressMessage::Start {
            message: message.into(),
        })
        .await
    }

    /// Update the progress bar message
    pub async fn update(
        &self,
        message: impl Into<String>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<ProgressMessage>> {
        self.send(ProgressMessage::Update {
            message: message.into(),
        })
        .await
    }

    /// Finish the progress bar with a success message
    pub async fn finish(
        &self,
        message: impl Into<String>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<ProgressMessage>> {
        self.send(ProgressMessage::Finish {
            message: message.into(),
        })
        .await
    }

    /// Finish the progress bar with an error message
    pub async fn finish_with_error(
        &self,
        message: impl Into<String>,
    ) -> Result<(), tokio::sync::mpsc::error::SendError<ProgressMessage>> {
        self.send(ProgressMessage::FinishWithError {
            message: message.into(),
        })
        .await
    }

    /// Manually shutdown the progress bar (usually not needed due to Drop)
    pub async fn shutdown(mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.tx.send(ProgressMessage::Shutdown).await?;

        if let Some(handle) = self.handle.take() {
            handle
                .join()
                .map_err(|_| "Failed to join progress thread")?;
        }

        Ok(())
    }
}

impl Drop for ProgressHandle {
    fn drop(&mut self) {
        // Send shutdown message (ignore errors as channel might be closed)
        let _ = self.tx.try_send(ProgressMessage::Shutdown);

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
            tracing::debug!(target: "app::util", "Dropped ProgessHandle");
        }
    }
}

/// Removes all files in the provided slice of paths.
/// Skips files that don't exist and logs any deletion errors
pub async fn remove_files(paths: &[impl AsRef<Path>]) {
    for path in paths {
        let path_ref = path.as_ref();
        const TARGET: &str = "zv::utils::remove_files";
        // Check if file exists before attempting to remove
        match tokio::fs::metadata(path_ref).await {
            Ok(metadata) => {
                // File exists, attempt to remove it
                if metadata.is_file() {
                    if let Err(e) = tokio::fs::remove_file(path_ref).await {
                        // Only log error if it's not a "file not found" error
                        // (in case file was deleted between metadata check and removal)
                        if e.kind() != std::io::ErrorKind::NotFound {
                            tracing::debug!(
                                target: TARGET,
                                "Failed to remove file '{}': {}",
                                path_ref.display(),
                                e
                            );
                        }
                    }
                }
            }
            Err(e) => {
                // If error is "not found", skip this file
                if e.kind() != std::io::ErrorKind::NotFound {
                    // For other metadata errors (permissions, etc.), log the error
                    tracing::debug!(
                        target: TARGET,
                        "Failed to access file '{}': {}",
                        path_ref.display(),
                        e
                    );
                }
                // File doesn't exist, skip it
            }
        }
    }
}

/// Verify SHA-256 checksum of a file
///
/// Reads the file and computes its SHA-256 hash, comparing it with the expected checksum.
/// Returns an error if the checksums don't match or if file reading fails.
/// Enhanced with comprehensive error handling and detailed logging for debugging.
pub(crate) async fn verify_checksum(
    file_path: &Path,
    expected_shasum: &str,
) -> Result<(), ZvError> {
    use tokio::io::AsyncReadExt;
    const TARGET: &str = "zv::utils::verify_checksum";
    tracing::debug!(target: TARGET, "Starting checksum verification for file: {}", file_path.display());
    tracing::debug!(target: TARGET, "Expected SHA-256: {}", expected_shasum);

    // Validate input parameters
    if expected_shasum.is_empty() {
        let error_msg = "Expected checksum is empty - cannot verify file integrity";
        tracing::error!(target: TARGET, "{}", error_msg);
        return Err(ZvError::General(eyre!(error_msg)));
    }

    if expected_shasum.len() != 64 {
        let error_msg = format!(
            "Expected checksum has invalid length {} (should be 64 hex characters for SHA-256): {}",
            expected_shasum.len(),
            expected_shasum
        );
        tracing::error!(target: TARGET, "{}", error_msg);
        return Err(ZvError::General(eyre!(error_msg)));
    }

    // Validate that expected checksum contains only hex characters
    if !expected_shasum.chars().all(|c| c.is_ascii_hexdigit()) {
        let error_msg = format!(
            "Expected checksum contains non-hexadecimal characters: {}",
            expected_shasum
        );
        tracing::error!(target: TARGET, "{}", error_msg);
        return Err(ZvError::General(eyre!(error_msg)));
    }

    // Check if file exists and get metadata
    let file_metadata = match tokio::fs::metadata(file_path).await {
        Ok(metadata) => {
            let file_size = metadata.len();
            tracing::debug!(target: TARGET, "File size: {} bytes ({:.1} MB)", file_size, file_size as f64 / 1_048_576.0);

            if file_size == 0 {
                tracing::warn!(target: TARGET, "File is empty - this may indicate a download failure");
            }

            metadata
        }
        Err(e) => {
            let error_msg = format!(
                "Failed to read file metadata for checksum verification: {}",
                file_path.display()
            );
            tracing::error!(target: TARGET, "{}: {}", error_msg, e);
            return Err(ZvError::General(eyre!("{}: {}", error_msg, e)));
        }
    };

    // Open the file for reading
    let mut file = match tokio::fs::File::open(file_path).await {
        Ok(file) => {
            tracing::debug!(target: TARGET, "Successfully opened file for checksum verification");
            file
        }
        Err(e) => {
            let error_msg = format!(
                "Failed to open file for checksum verification: {}",
                file_path.display()
            );
            tracing::error!(target: TARGET, "{}: {}", error_msg, e);

            // Provide specific error context
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    tracing::error!(target: TARGET, "File not found - it may have been deleted or moved during verification");
                }
                std::io::ErrorKind::PermissionDenied => {
                    tracing::error!(target: TARGET, "Permission denied - check file read permissions");
                }
                _ => {
                    tracing::error!(target: TARGET, "Unexpected I/O error opening file: {:?}", e.kind());
                }
            }

            return Err(ZvError::Io(e));
        }
    };

    // Create SHA-256 hasher
    let mut hasher = <Sha256 as Digest>::new();
    let mut buffer = [0u8; 8192]; // 8KB buffer for efficient reading
    let mut total_bytes_read = 0u64;
    let file_size = file_metadata.len();

    tracing::debug!(target: TARGET, "Starting SHA-256 computation with 8KB buffer");

    // Read file in chunks and update hasher
    loop {
        let bytes_read = match file.read(&mut buffer).await {
            Ok(bytes) => bytes,
            Err(e) => {
                let error_msg = format!(
                    "Failed to read file during checksum verification: {}",
                    file_path.display()
                );
                tracing::error!(target: TARGET, "{}: {}", error_msg, e);

                // Provide specific error context
                match e.kind() {
                    std::io::ErrorKind::UnexpectedEof => {
                        tracing::error!(target: TARGET, "Unexpected end of file - file may be truncated or corrupted");
                    }
                    std::io::ErrorKind::Interrupted => {
                        tracing::warn!(target: TARGET, "Read operation interrupted - this is usually recoverable");
                        continue; // Retry the read operation
                    }
                    _ => {
                        tracing::error!(target: TARGET, "Unexpected I/O error during file read: {:?}", e.kind());
                    }
                }

                return Err(ZvError::Io(e));
            }
        };

        if bytes_read == 0 {
            tracing::debug!(target: TARGET, "Reached end of file after reading {} bytes", total_bytes_read);
            break; // End of file
        }

        hasher.update(&buffer[..bytes_read]);
        total_bytes_read += bytes_read as u64;
    }

    // Verify we read the expected amount of data
    if total_bytes_read != file_size {
        let error_msg = format!(
            "File size mismatch during checksum verification: expected {} bytes, read {} bytes",
            file_size, total_bytes_read
        );
        tracing::error!(target: TARGET, "{}", error_msg);
        return Err(ZvError::General(eyre!(error_msg)));
    }

    // Finalize hash and convert to hex string
    let computed_hash = hasher.finalize();
    let computed_hex = format!("{:x}", computed_hash);

    tracing::debug!(target: TARGET, "Computed SHA-256: {}", computed_hex);
    tracing::debug!(target: TARGET, "Checksum computation completed for {} bytes", total_bytes_read);

    // Compare with expected checksum (case-insensitive)
    if computed_hex.eq_ignore_ascii_case(expected_shasum) {
        tracing::trace!(target: TARGET, "Checksum verification successful for file: {} ({:.1} MB)", 
                      file_path.display(), total_bytes_read as f64 / 1_048_576.0);
        Ok(())
    } else {
        let error_msg = format!(
            "Checksum verification failed for file: {}\nFile size: {} bytes ({:.1} MB)\nExpected SHA-256: {}\nComputed SHA-256: {}\nThis indicates file corruption or an incorrect expected checksum",
            file_path.display(),
            total_bytes_read,
            total_bytes_read as f64 / 1_048_576.0,
            expected_shasum,
            computed_hex
        );
        tracing::error!(target: TARGET, "CHECKSUM MISMATCH: {}", error_msg);

        // Additional debugging information
        tracing::error!(target: TARGET, "Checksum verification details:");
        tracing::error!(target: TARGET, "  File: {}", file_path.display());
        tracing::error!(target: TARGET, "  Size: {} bytes", total_bytes_read);
        tracing::error!(target: TARGET, "  Expected: {}", expected_shasum);
        tracing::error!(target: TARGET, "  Computed: {}", computed_hex);
        tracing::error!(target: TARGET, "  This may indicate network corruption, storage issues, or incorrect metadata");

        Err(ZvError::General(eyre!(error_msg)))
    }
}
