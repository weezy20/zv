use crate::{
    ZigVersion, ZvError,
    tools::canonicalize,
    types::{ArchiveExt, Shim},
};
use color_eyre::eyre::eyre;
use indicatif::{ProgressBar, ProgressStyle};
use same_file::Handle;
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
pub fn zig_tarball(zig_version: &ZigVersion, extension: Option<ArchiveExt>) -> Option<String> {
    use target_lexicon::HOST;
    // Return None for Unknown variant
    let semver_version = zig_version.version();

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
    if let Some(v) = semver_version {
        if v.le(&semver::Version::new(0, 14, 0)) {
            return Some(format!("zig-{os}-{arch}-{v}.{ext}"));
        } else {
            return Some(format!("zig-{arch}-{os}-{v}.{ext}"));
        }
    }
    None
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
