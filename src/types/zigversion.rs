use crate::ZvError;
use color_eyre::eyre::eyre;
const TARGET: &'static str = "zv::zig_version";
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{
    fmt,
    hash::{Hash, Hasher},
    path::PathBuf,
    str::FromStr,
};

#[derive(Debug, Clone)]
/// A type denoting a valid zig version
pub enum ZigVersion {
    /// Semantic version
    Semver(Version),
    /// Master branch build
    Master(Version),
    /// System-installed Zig (Non-zv managed if any)
    System {
        path: Option<PathBuf>,
        version: Option<Version>,
    },
    /// Latest stable (cached)
    Stable(Version),
    /// Latest stable (always refresh)
    Latest(Version),
    /// Unknown - Detected but failed to execute `zig version`
    Unknown,
}

impl ZigVersion {
    /// Creates a placeholder version (0.0.0) for the given variant type
    fn placeholder_for_variant(variant: &str) -> Result<Self, ZvError> {
        let placeholder = Version::parse("0.0.0").unwrap();
        match variant {
            "master" => Ok(ZigVersion::Master(placeholder)),
            "stable" => Ok(ZigVersion::Stable(placeholder)),
            "latest" => Ok(ZigVersion::Latest(placeholder)),
            "system" => Ok(ZigVersion::System {
                version: None,
                path: None,
            }),
            _ => Err(ZvError::General(eyre!("Invalid variant: {}", variant))),
        }
    }

    /// Normalizes a version string to semver format (e.g., "1" -> "1.0.0", "1.2" -> "1.2.0")
    fn normalize_version_string(version_str: &str) -> String {
        match version_str.chars().filter(|&c| c == '.').count() {
            0 => format!("{}.0.0", version_str),
            1 => format!("{}.0", version_str),
            _ => version_str.to_string(),
        }
    }

    /// Parses a version string with normalization
    fn parse_normalized_version(version_str: &str) -> Result<Version, ZvError> {
        let normalized = Self::normalize_version_string(version_str);
        Version::parse(&normalized).map_err(ZvError::ZigVersionError)
    }

    /// Extracts the version from any ZigVersion variant, if available
    pub fn version(&self) -> Option<&Version> {
        match self {
            ZigVersion::Semver(v)
            | ZigVersion::Master(v)
            | ZigVersion::Stable(v)
            | ZigVersion::Latest(v) => Some(v),
            ZigVersion::System { version, .. } => version.as_ref(),
            ZigVersion::Unknown => None,
        }
    }

    /// Returns true if embedded version is a placeholder (0.0.0)
    /// Returns false in all other cases
    pub fn is_placeholder_version(&self) -> bool {
        if let ZigVersion::System { version, path } = self {
            if path.is_none() && version.is_none() {
                // System variant with unknown version is not a placeholder
                return true;
            }
        }
        let placeholder = Version::parse("0.0.0").unwrap();
        self.version().map_or(false, |v| *v == placeholder)
    }

    /// Returns true if the versions match, ignoring variant differences and paths for System variants.
    /// This provides version-only comparison logic that was used in the old PartialEq implementation.
    pub fn version_matches(&self, other: &Self) -> bool {
        match (self.version(), other.version()) {
            (Some(v1), Some(v2)) => v1 == v2,
            (None, None) => matches!((self, other), (ZigVersion::Unknown, ZigVersion::Unknown)),
            _ => false,
        }
    }

    /// Returns true if this is a system variant
    pub fn is_system(&self) -> bool {
        matches!(self, ZigVersion::System { .. })
    }

    /// Returns the path for system variants
    pub fn system_path(&self) -> Option<&PathBuf> {
        match self {
            ZigVersion::System { path, .. } => path.as_ref(),
            _ => None,
        }
    }
}

impl FromStr for ZigVersion {
    type Err = ZvError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "unknown" => Err(ZvError::General(eyre!(
                "`unknown` is not a valid user input"
            ))),
            "master" => Self::placeholder_for_variant("master"),
            "system" => Self::placeholder_for_variant("system"),
            "stable" => Self::placeholder_for_variant("stable"),
            "latest" => Self::placeholder_for_variant("latest"),
            _ => {
                // Handle prefixed variants (system@version, stable@version)
                if let Some((prefix, version_str)) = s.split_once('@') {
                    let version = Self::parse_normalized_version(version_str)?;
                    return match prefix {
                        "system" => Ok(ZigVersion::System {
                            path: None,
                            version: Some(version),
                        }),
                        "stable" => Ok(ZigVersion::Semver(version)),
                        _ => Err(ZvError::General(eyre!(
                            "Invalid version prefix: {}",
                            prefix
                        ))),
                    };
                }
                // Parse as direct semver if it starts with a digit
                if s.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    Self::parse_normalized_version(s).map(ZigVersion::Semver)
                } else {
                    Err(ZvError::General(eyre!(
                        "Not a valid Zig version string: {}",
                        s
                    )))
                }
            }
        }
    }
}

impl Hash for ZigVersion {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            // Use same discriminant for variants that should hash equally when versions match
            ZigVersion::Semver(v)
            | ZigVersion::Master(v)
            | ZigVersion::Stable(v)
            | ZigVersion::Latest(v) => {
                state.write_u8(0);
                v.hash(state);
            }
            ZigVersion::System { version, path } => {
                state.write_u8(1); // Different discriminant since path matters for equality
                version.hash(state);
                path.hash(state);
            }
            ZigVersion::Unknown => {
                state.write_u8(2);
            }
        }
    }
}

impl PartialEq for ZigVersion {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            // System variants: both path and version must match
            (
                Self::System {
                    path: l_path,
                    version: l_version,
                },
                Self::System {
                    path: r_path,
                    version: r_version,
                },
            ) => *l_path == *r_path && *l_version == *r_version,

            // Unknown only equals Unknown
            (Self::Unknown, Self::Unknown) => true,
            (Self::Unknown, _) | (_, Self::Unknown) => false,

            // System vs non-System: always false (they're fundamentally different)
            (Self::System { .. }, _) | (_, Self::System { .. }) => false,

            // All other variants: compare versions only
            (l, r) => match (l.version(), r.version()) {
                (Some(lv), Some(rv)) => *lv == *rv,
                _ => false,
            },
        }
    }
}

impl Eq for ZigVersion {}

impl Serialize for ZigVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use std::collections::BTreeMap;

        match self {
            ZigVersion::Semver(version) => {
                let mut map = BTreeMap::new();
                map.insert("version", version.to_string());
                map.serialize(serializer)
            }
            ZigVersion::Master(version) => {
                let mut map = BTreeMap::new();
                map.insert("master", version.to_string());
                map.serialize(serializer)
            }
            ZigVersion::Stable(version) => {
                let mut map = BTreeMap::new();
                map.insert("version", version.to_string());
                map.serialize(serializer)
            }
            ZigVersion::Latest(version) => {
                let mut map = BTreeMap::new();
                map.insert("version", version.to_string());
                map.serialize(serializer)
            }
            ZigVersion::System { path, version } => {
                let mut map = BTreeMap::new();
                map.insert(
                    "version",
                    version
                        .as_ref()
                        .map_or_else(|| "unknown".to_string(), |v| v.to_string()),
                );
                map.insert(
                    "path",
                    path.as_ref()
                        .map_or_else(String::new, |p| p.display().to_string()),
                );
                map.serialize(serializer)
            }
            ZigVersion::Unknown => serializer.serialize_str("unknown"),
        }
    }
}

impl<'de> Deserialize<'de> for ZigVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ZigVersionHelper {
            String(String),
            Map(std::collections::BTreeMap<String, String>),
        }

        let helper = ZigVersionHelper::deserialize(deserializer)?;

        match helper {
            ZigVersionHelper::String(s) => {
                if s == "unknown" {
                    Ok(ZigVersion::Unknown)
                } else {
                    ZigVersion::from_str(&s).map_err(de::Error::custom)
                }
            }
            ZigVersionHelper::Map(map) => {
                // Handle specific variant keys first
                if let Some(master_str) = map.get("master") {
                    let version = Version::parse(master_str).map_err(de::Error::custom)?;
                    return Ok(ZigVersion::Master(version));
                }

                if let Some(stable_str) = map.get("stable") {
                    let version = Version::parse(stable_str).map_err(de::Error::custom)?;
                    return Ok(ZigVersion::Stable(version));
                }

                if let Some(latest_str) = map.get("latest") {
                    let version = Version::parse(latest_str).map_err(de::Error::custom)?;
                    return Ok(ZigVersion::Latest(version));
                }

                // Handle generic "version" key
                if let Some(version_str) = map.get("version") {
                    // If there's also a "path" key, it's a System variant
                    if map.contains_key("path") {
                        return Self::deserialize_system_variant(&map).map_err(de::Error::custom);
                    } else {
                        // Only "version" key - treat as Semver for backward compatibility
                        return Self::deserialize_version_only(version_str)
                            .map_err(de::Error::custom);
                    }
                }

                // Handle case where only path is present (System variant with unknown version)
                if map.contains_key("path") {
                    return Self::deserialize_system_variant(&map).map_err(de::Error::custom);
                }

                Err(de::Error::custom(
                    "Invalid version structure: no recognized keys found",
                ))
            }
        }
    }
}

impl ZigVersion {
    /// Helper for deserializing System variants
    fn deserialize_system_variant(
        map: &std::collections::BTreeMap<String, String>,
    ) -> Result<Self, ZvError> {
        let version = map
            .get("version")
            .and_then(|v| {
                if v == "unknown" || v.is_empty() {
                    None
                } else {
                    Some(v)
                }
            })
            .map(|v| Version::parse(v))
            .transpose()
            .map_err(|err| {
                tracing::error!(target: TARGET,
                    "Failed to parse version string during deserialization: {}",
                    err
                );
                ZvError::ZigVersionError(err)
            })?;

        let path = map.get("path").and_then(|p| {
            if p == "unknown" || p.is_empty() {
                None
            } else {
                Some(PathBuf::from(p))
            }
        });

        Ok(ZigVersion::System { version, path })
    }

    /// Helper for deserializing version-only entries
    fn deserialize_version_only(version_str: &str) -> Result<Self, ZvError> {
        if version_str == "unknown" || version_str.is_empty() {
            Ok(ZigVersion::System {
                version: None,
                path: None,
            })
        } else {
            let version = Version::parse(version_str).map_err(ZvError::ZigVersionError)?;
            Ok(ZigVersion::Semver(version))
        }
    }
}

impl fmt::Display for ZigVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZigVersion::Semver(v) => write!(f, "{}", v),
            ZigVersion::Master(v) => write!(f, "master <{}>", v),
            ZigVersion::System { version, path } => {
                let version_str = version
                    .as_ref()
                    .map_or_else(|| "unknown".to_string(), |v| v.to_string());
                let path_str = path
                    .as_ref()
                    .map_or_else(|| "unknown".to_string(), |p| p.display().to_string());
                write!(f, "system <{}> [<{}>]", version_str, path_str)
            }
            ZigVersion::Stable(v) => write!(f, "stable <{}>", v),
            ZigVersion::Latest(v) => write!(f, "latest <{}>", v),
            ZigVersion::Unknown => write!(f, "unknown"),
        }
    }
}
