use crate::{CfgErr, ZigVersion, ZvError};
use ahash::AHashMap;
use chrono::{DateTime, Utc};
use color_eyre::eyre::eyre;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use toml_edit::{DocumentMut, InlineTable, Item, Value};

const ZV_CONFIG_TARGET: &'static str = "zv_config";

/// The actual config data structure that gets put into config.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZvConfigData {
    /// Current active Zig Version
    pub active_version: Option<ZigVersion>,

    /// System detected Zig installations - non zv managed installations found in $PATH
    #[serde(rename = "system_detected_zig")]
    pub system_detected: Vec<ZigVersion>,

    /// Zv managed versions and their installation paths
    #[serde(default)]
    pub zig: AHashMap<semver::Version, ZigEntry>,

    /// Path to the config file
    #[serde(skip)]
    pub config_path: PathBuf,
}

/// Zig installation configuration stored in `config.toml`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZigEntry {
    /// Installation path
    pub path: PathBuf,
    /// Checksum
    #[serde(skip_serializing_if = "String::is_empty")]
    pub checksum: String,
    /// Checksum verified
    pub checksum_verified: bool,
    /// Minisig Verified
    pub minisig_verified: bool,
    /// Download URL used to obtain this version
    #[serde(skip_serializing_if = "String::is_empty")]
    pub download_url: String,
    /// Download Timestamp
    pub downloaded_at: DateTime<Utc>,
}

impl ZigEntry {
    pub fn from_directory(path: PathBuf) -> Self {
        let timestamp = get_directory_timestamps(&path);

        Self {
            path,
            checksum: String::new(),
            checksum_verified: false,
            minisig_verified: false,
            download_url: String::new(),
            downloaded_at: timestamp.unwrap_or_else(Utc::now),
        }
    }
}

/// Returns dir creation/modification timestamp - When zv is scanning a new directory it doesn't know about its status
fn get_directory_timestamps(path: &std::path::Path) -> Option<DateTime<Utc>> {
    let metadata = std::fs::metadata(path).ok()?;

    let created = metadata.created().ok().map(DateTime::from);
    let modified = metadata.modified().ok().map(DateTime::from);

    // Check for potential tampering - if both times exist and differ significantly
    let potentially_tampered = match (created, modified) {
        (Some(c), Some(m)) => {
            // Consider it tampered if times differ by more than a small threshold
            // (accounting for filesystem timestamp precision)
            let diff = (c - m).abs();
            diff > chrono::Duration::seconds(1) // 1 second 
        }
        _ => false,
    };
    if potentially_tampered {
        tracing::warn!(target: ZV_CONFIG_TARGET, "Difference detected between created and modified timestamps");
        tracing::warn!(target: ZV_CONFIG_TARGET, "Created: {:?}, Modified: {:?}", created, modified);
        tracing::warn!(target: ZV_CONFIG_TARGET, "Path: {:?}", path);
    }
    created.or(modified)
}

impl ZvConfigData {
    /// Create a new ZvConfigData with the given config path
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            config_path,
            ..Default::default()
        }
    }

    // Active version management
    /// Returns the currently active version, if any
    pub fn get_active_version(&self) -> Option<&ZigVersion> {
        self.active_version.as_ref()
    }

    /// Set current active version, optionally saving to disk
    pub fn set_active_version(
        &mut self,
        zig_version: &ZigVersion,
        save_to_disk: bool,
    ) -> Result<(), ZvError> {
        self.active_version = Some(zig_version.to_owned());

        // If save_to_disk is true, save the active version to disk
        if save_to_disk {
            self.save_active_version()?;
        }

        Ok(())
    }

    // Persistence operations
    /// Save the entire config to disk
    pub fn save(&self) -> Result<(), ZvError> {
        let contents = toml::to_string(self)
            .map_err(|err| ZvError::ZvConfigError(CfgErr::SerializeFail(err)))?;
        std::fs::write(&self.config_path, contents).map_err(ZvError::Io)?;
        Ok(())
    }

    /// Surgically update only the active_version field in the config file
    pub fn save_active_version(&self) -> Result<(), ZvError> {
        tracing::info!(target: ZV_CONFIG_TARGET, "Saving active version to config: {:?}", self.active_version);

        // Read the current config file
        let content = std::fs::read_to_string(&self.config_path).map_err(ZvError::Io)?;

        // Parse as editable TOML document
        let mut doc = content.parse::<DocumentMut>().map_err(|err| {
            ZvError::ZvConfigError(CfgErr::ParseFail(color_eyre::eyre::eyre!(err.to_string())))
        })?;

        // Update the active_version field
        if let Some(version) = &self.active_version {
            let version_value = match version {
                ZigVersion::Semver(v) => {
                    let mut table = InlineTable::new();
                    table.insert("version", Value::from(v.to_string()));
                    Value::InlineTable(table)
                }
                ZigVersion::Master(v) => {
                    let mut table = InlineTable::new();
                    table.insert("master", Value::from(v.to_string()));
                    Value::InlineTable(table)
                }
                ZigVersion::System { path, version } => {
                    // Ensure both version and path are Some, else return error
                    let version = match version {
                        Some(v) => v,
                        None => {
                            return Err(ZvError::ZvConfigError(CfgErr::WriteFail(eyre!(
                                "System ZigVersion missing version"
                            ))));
                        }
                    };
                    let path = match path {
                        Some(p) => p,
                        None => {
                            return Err(ZvError::ZvConfigError(CfgErr::WriteFail(eyre!(
                                "System ZigVersion missing path"
                            ))));
                        }
                    };
                    let mut table = InlineTable::new();
                    table.insert("version", Value::from(version.to_string()));
                    table.insert("path", Value::from(path.to_string_lossy().to_string()));
                    Value::InlineTable(table)
                }
                ZigVersion::Stable(v) => {
                    let mut table = InlineTable::new();
                    table.insert("stable", Value::from(v.to_string()));
                    Value::InlineTable(table)
                }
                ZigVersion::Latest(v) => {
                    let mut table = InlineTable::new();
                    table.insert("latest", Value::from(v.to_string()));
                    Value::InlineTable(table)
                }
                ZigVersion::Unknown => Value::from("unknown"),
            };

            doc["active_version"] = Item::Value(version_value);
            // Ensure proper spacing around the equals sign for the key
            if let Some(mut key) = doc.key_mut("active_version") {
                key.leaf_decor_mut().set_suffix(" ");
            }
        } else {
            // Set to null if no active version
            doc["active_version"] = Item::Value(Value::from(""));
            // Ensure proper spacing around the equals sign for the key
            if let Some(mut key) = doc.key_mut("active_version") {
                key.leaf_decor_mut().set_suffix(" ");
            }
        }

        // Write back to file
        std::fs::write(&self.config_path, doc.to_string())
            .map_err(|io_err| ZvError::ZvConfigError(CfgErr::WriteFail(eyre!(io_err))))?;

        Ok(())
    }
}

/// Implementation of ZvConfig trait with lazy loading and shared reference counting
pub struct ZvConfig {
    inner: Rc<RefCell<Option<ZvConfigData>>>,
    config_path: PathBuf,
}

impl Clone for ZvConfig {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            config_path: self.config_path.clone(),
        }
    }
}

impl ZvConfig {
    pub fn new(config_path: PathBuf) -> Self {
        Self {
            inner: Rc::new(RefCell::new(None)),
            config_path,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.borrow().is_some()
    }

    /// Load config if not already loaded (similar to App::load_config)
    fn ensure_loaded(&self) -> Result<(), ZvError> {
        if self.is_loaded() {
            return Ok(());
        }

        // Load config using existing logic from App::load_config
        let cfg_data = match std::fs::read_to_string(&self.config_path) {
            Ok(content) => match toml::de::from_str::<ZvConfigData>(&content) {
                Ok(mut cfg) => {
                    cfg.config_path = self.config_path.clone();
                    cfg
                }
                Err(_) => {
                    // Build new config if parsing fails
                    self.build_config()?
                }
            },
            Err(_) => {
                // Build new config if file doesn't exist
                self.build_config()?
            }
        };

        *self.inner.borrow_mut() = Some(cfg_data);
        Ok(())
    }

    /// Build new config (similar to App::build_config)
    fn build_config(&self) -> Result<ZvConfigData, ZvError> {
        // This would need access to App methods like system_zig() and scan_zv_zig()
        // For now, return a minimal config - this will be refined in implementation
        Ok(ZvConfigData {
            active_version: None,
            system_detected: vec![],
            zig: AHashMap::new(),
            config_path: self.config_path.clone(),
        })
    }

    // Active version management
    pub fn get_active_version(&self) -> Result<Option<ZigVersion>, ZvError> {
        self.ensure_loaded()?;
        Ok(self.inner.borrow().as_ref().unwrap().active_version.clone())
    }

    pub fn set_active_version(
        &self,
        version: &ZigVersion,
        save_to_disk: bool,
    ) -> Result<(), ZvError> {
        self.ensure_loaded()?;
        {
            let mut cfg = self.inner.borrow_mut();
            cfg.as_mut().unwrap().active_version = Some(version.clone());
        }

        if save_to_disk {
            self.save_active_version()
        } else {
            Ok(())
        }
    }

    // System detected versions management
    pub fn get_system_detected(&self) -> Result<Vec<ZigVersion>, ZvError> {
        self.ensure_loaded()?;
        Ok(self
            .inner
            .borrow()
            .as_ref()
            .unwrap()
            .system_detected
            .clone())
    }

    pub fn add_system_detected(&self, version: ZigVersion) -> Result<(), ZvError> {
        self.ensure_loaded()?;
        let mut cfg = self.inner.borrow_mut();
        let cfg_data = cfg.as_mut().unwrap();
        if !cfg_data.system_detected.contains(&version) {
            cfg_data.system_detected.push(version);
        }
        Ok(())
    }

    pub fn resync_system_detected(&self, system_versions: Vec<ZigVersion>) -> Result<(), ZvError> {
        self.ensure_loaded()?;
        let mut cfg = self.inner.borrow_mut();
        cfg.as_mut().unwrap().system_detected = system_versions;
        Ok(())
    }

    // ZV-managed Zig installations
    pub fn get_zv_zig(&self) -> Result<AHashMap<ZigVersion, ZigEntry>, ZvError> {
        self.ensure_loaded()?;
        Ok(self.inner.borrow().as_ref().unwrap().zig.clone())
    }

    pub fn add_zv_zig(&self, version: ZigVersion, entry: ZigEntry) -> Result<(), ZvError> {
        self.ensure_loaded()?;
        let mut cfg = self.inner.borrow_mut();
        cfg.as_mut().unwrap().zig.insert(version, entry);
        Ok(())
    }

    // Persistence operations
    pub fn save(&self) -> Result<(), ZvError> {
        self.ensure_loaded()?;
        self.inner.borrow().as_ref().unwrap().save()
    }

    pub fn save_active_version(&self) -> Result<(), ZvError> {
        self.ensure_loaded()?;
        let cfg = self.inner.borrow();
        let cfg_data = cfg.as_ref().unwrap();

        // Check if the active version is a placeholder version
        if let Some(version) = &cfg_data.active_version {
            if version.is_placeholder_version() {
                return Err(ZvError::ZvConfigError(CfgErr::WriteFail(eyre!(
                    "Cannot save placeholder version (0.0.0) as active version"
                ))));
            }
        }

        cfg_data.save_active_version()
    }
}
