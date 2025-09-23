use std::path::Path;
use yansi::Paint;
/// Windows PATH Manager for registry-based PATH management
#[cfg(windows)]
pub struct WindowsPathManager {
    environment_key: windows_registry::Key,
}

#[cfg(windows)]
impl WindowsPathManager {
    /// Create a new Windows PATH manager
    pub fn new() -> crate::Result<Self> {
        use windows_registry::CURRENT_USER;
        let environment_key = CURRENT_USER.create("Environment").map_err(|e| {
            crate::ZvError::shell_registry_failed(&format!(
                "Failed to open Environment registry key: {}",
                e
            ))
        })?;
        Ok(Self { environment_key })
    }

    /// Add a path to the user's PATH environment variable
    pub fn add_to_path(&self, new_path: &str) -> crate::Result<()> {
        let current_path = self.get_current_path()?;
        let new_path_value = match current_path {
            Some(existing) if !self.path_contains(&existing, new_path) => {
                format!("{};{}", new_path, existing)
            }
            None => new_path.to_string(),
            Some(_) => return Ok(()), // Already in path
        };

        self.environment_key
            .set_string("PATH", &new_path_value)
            .map_err(|e| {
                crate::ZvError::shell_path_operation_failed(&format!(
                    "Failed to set PATH in registry: {}",
                    e
                ))
            })?;

        self.broadcast_environment_change()?;
        Ok(())
    }

    /// Remove a path from the user's PATH environment variable
    pub fn remove_from_path(&self, target_path: &str) -> crate::Result<bool> {
        let current_path = match self.get_current_path()? {
            Some(path) => path,
            None => return Ok(false), // No PATH set, nothing to remove
        };

        if !self.path_contains(&current_path, target_path) {
            return Ok(false); // Path not found, nothing to remove
        }

        let new_path_value: Vec<&str> = current_path
            .split(';')
            .filter(|p| !p.trim().eq_ignore_ascii_case(target_path.trim()))
            .collect();

        let new_path_string = new_path_value.join(";");

        self.environment_key
            .set_string("PATH", &new_path_string)
            .map_err(|e| {
                crate::ZvError::shell_path_operation_failed(&format!(
                    "Failed to update PATH in registry: {}",
                    e
                ))
            })?;

        self.broadcast_environment_change()?;
        Ok(true)
    }

    /// Get the current PATH value from registry
    fn get_current_path(&self) -> crate::Result<Option<String>> {
        match self.environment_key.get_string("PATH") {
            Ok(path) => Ok(Some(path)),
            Err(_) => Ok(None), // PATH not set in user registry
        }
    }

    /// Check if PATH contains the specified path
    fn path_contains(&self, path_value: &str, target_path: &str) -> bool {
        // Handle empty strings
        if path_value.is_empty() || target_path.is_empty() {
            return false;
        }

        path_value.split(';').any(|p| {
            let trimmed = p.trim();
            !trimmed.is_empty() && trimmed.eq_ignore_ascii_case(target_path.trim())
        })
    }

    /// Broadcast environment variable changes to notify running applications
    fn broadcast_environment_change(&self) -> crate::Result<()> {
        broadcast_environment_change()
    }

    /// Set an environment variable in the Windows registry
    pub fn set_environment_variable(&self, name: &str, value: &str) -> crate::Result<()> {
        self.environment_key.set_string(name, value).map_err(|e| {
            crate::ZvError::shell_registry_failed(&format!(
                "Failed to set {} in registry: {}",
                name, e
            ))
        })?;

        self.broadcast_environment_change()?;
        Ok(())
    }

    /// Get an environment variable from the Windows registry
    pub fn get_environment_variable(&self, name: &str) -> crate::Result<Option<String>> {
        match self.environment_key.get_string(name) {
            Ok(value) => Ok(Some(value)),
            Err(_) => Ok(None), // Variable not set in user registry
        }
    }

    /// Remove an environment variable from the Windows registry
    pub fn remove_environment_variable(&self, name: &str) -> crate::Result<bool> {
        match self.environment_key.remove_value(name) {
            Ok(_) => {
                self.broadcast_environment_change()?;
                Ok(true)
            }
            Err(_) => Ok(false), // Variable not found
        }
    }
}

/// Broadcast environment variable changes on Windows
#[cfg(windows)]
pub fn broadcast_environment_change() -> crate::Result<()> {
    use std::ptr;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutA, WM_SETTINGCHANGE,
    };

    // Tell other processes to update their environment
    #[allow(clippy::unnecessary_cast)]
    unsafe {
        SendMessageTimeoutA(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0 as WPARAM,
            c"Environment".as_ptr() as LPARAM,
            SMTO_ABORTIFHUNG,
            5000,
            ptr::null_mut(),
        );
    }

    Ok(())
}

/// Execute PATH setup on Windows using registry
#[cfg(windows)]
pub async fn execute_path_setup_windows(
    context: &crate::shell::setup::SetupContext,
    bin_path: &Path,
) -> crate::Result<()> {
    let path_manager = WindowsPathManager::new()?;
    let bin_path_str = bin_path.to_string_lossy().to_string();

    path_manager.add_to_path(&bin_path_str)?;

    println!(
        "✓ Added {} to PATH in Windows registry",
        Paint::green(&bin_path_str)
    );

    // Track the registry modification
    use crate::shell::setup::instructions::create_registry_entry;
    context.add_modified_file(create_registry_entry());

    Ok(())
}

/// Execute ZV_DIR setup on Windows using registry
#[cfg(windows)]
pub async fn execute_zv_dir_setup_windows(zv_dir: &Path) -> crate::Result<()> {
    let path_manager = WindowsPathManager::new()?;
    let zv_dir_str = zv_dir.to_string_lossy().to_string();

    path_manager.set_environment_variable("ZV_DIR", &zv_dir_str)?;

    println!(
        "✓ ZV_DIR set to {} in Windows registry",
        Paint::green(&zv_dir_str)
    );
    Ok(())
}

/// Check if ZV_DIR is permanently set on Windows
#[cfg(windows)]
pub async fn check_zv_dir_permanent_windows(zv_dir: &Path) -> crate::Result<bool> {
    let path_manager = WindowsPathManager::new()?;

    match path_manager.get_environment_variable("ZV_DIR")? {
        Some(registry_value) => {
            let registry_path = Path::new(&registry_value);
            // Compare canonical paths if possible
            match (registry_path.canonicalize(), zv_dir.canonicalize()) {
                (Ok(reg_canonical), Ok(zv_canonical)) => Ok(reg_canonical == zv_canonical),
                _ => Ok(registry_path == zv_dir),
            }
        }
        None => Ok(false), // ZV_DIR not set in registry
    }
}

/// Check if a path is in the Windows PATH environment variable
#[cfg(windows)]
pub fn check_path_in_windows_path(target_path: &Path) -> crate::Result<bool> {
    let path_manager = WindowsPathManager::new()?;
    let target_path_str = target_path.to_string_lossy();

    match path_manager.get_current_path()? {
        Some(current_path) => Ok(path_manager.path_contains(&current_path, &target_path_str)),
        None => Ok(false),
    }
}

// Placeholder implementations for non-Windows platforms
#[cfg(not(windows))]
pub struct WindowsPathManager;

#[cfg(not(windows))]
impl WindowsPathManager {
    pub fn new() -> crate::Result<Self> {
        unreachable!("WindowsPathManager should not be used on non-Windows platforms")
    }
}

#[cfg(not(windows))]
pub fn check_path_in_windows_path(_target_path: &Path) -> crate::Result<bool> {
    unreachable!("Windows PATH check should not be called on non-Windows platforms")
}
