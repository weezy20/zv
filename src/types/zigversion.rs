use crate::ZvError;
use color_eyre::eyre::eyre;
use semver::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{
    fmt,
    hash::{Hash, Hasher},
    str::FromStr,
};

#[derive(Debug, Clone)]
/// A type denoting a valid zig version
pub enum ZigVersion {
    /// Semantic version
    Semver(Version),
    /// Master branch build
    Master(Version),
    /// Latest stable (cached)
    Stable(Version),
    /// Latest stable (always refresh)
    Latest(Version),
    /// Unknown - Detected but failed to execute `zig version`
    Unknown,
}

impl ZigVersion {
    /// Creates a placeholder version (0.0.0) for the given variant type
    pub fn placeholder_for_variant(variant: &str) -> Result<Self, ZvError> {
        let placeholder = Version::parse("0.0.0").unwrap();
        match variant {
            "master" => Ok(ZigVersion::Master(placeholder)),
            "stable" => Ok(ZigVersion::Stable(placeholder)),
            "latest" => Ok(ZigVersion::Latest(placeholder)),
            _ => Err(ZvError::General(eyre!("Invalid variant: {}", variant))),
        }
    }

    /// Normalizes a version string to semver format (e.g., "1" -> "1.0.0", "1.2" -> "1.2.0")
    /// Returns parsed version after normalization
    fn parse_normalized_version(version_str: &str) -> Result<Version, ZvError> {
        let normalized = match version_str.chars().filter(|&c| c == '.').count() {
            0 => format!("{}.0.0", version_str),
            1 => format!("{}.0", version_str),
            _ => version_str.to_string(),
        };
        Version::parse(&normalized).map_err(ZvError::ZigVersionError)
    }

    /// Extracts the version from any ZigVersion variant, if available and not a placeholder
    pub fn version(&self) -> Option<&Version> {
        match self {
            ZigVersion::Semver(v)
            | ZigVersion::Master(v)
            | ZigVersion::Stable(v)
            | ZigVersion::Latest(v)
                if *v != Version::new(0, 0, 0) =>
            {
                Some(v)
            }
            _ => None,
        }
    }

    /// Returns true if embedded version is a placeholder (0.0.0)
    /// Returns false in all other cases
    pub fn is_placeholder_version(&self) -> bool {
        match self {
            ZigVersion::Semver(v)
            | ZigVersion::Master(v)
            | ZigVersion::Stable(v)
            | ZigVersion::Latest(v) => *v == Version::new(0, 0, 0),
            ZigVersion::Unknown => false,
        }
    }

    /// Compare inner semvers for different zig version variants
    pub fn version_matches(&self, other: &Self) -> bool {
        match (self.version(), other.version()) {
            (Some(v1), Some(v2)) => v1 == v2,
            (None, None) => {
                // Both are None - could be Unknown or placeholder versions
                match (self, other) {
                    (ZigVersion::Unknown, ZigVersion::Unknown) => true,
                    // If both are placeholder versions (not Unknown), they match if they're the same variant
                    _ if self.is_placeholder_version() && other.is_placeholder_version() => {
                        // Extract raw versions for comparison
                        let self_raw = match self {
                            ZigVersion::Semver(v)
                            | ZigVersion::Master(v)
                            | ZigVersion::Stable(v)
                            | ZigVersion::Latest(v) => Some(v),
                            ZigVersion::Unknown => None,
                        };
                        let other_raw = match other {
                            ZigVersion::Semver(v)
                            | ZigVersion::Master(v)
                            | ZigVersion::Stable(v)
                            | ZigVersion::Latest(v) => Some(v),
                            ZigVersion::Unknown => None,
                        };
                        match (self_raw, other_raw) {
                            (Some(v1), Some(v2)) => v1 == v2,
                            _ => false,
                        }
                    }
                    _ => false,
                }
            }
            _ => false,
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
            "stable" => Self::placeholder_for_variant("stable"),
            "latest" => Self::placeholder_for_variant("latest"),
            _ => {
                // Handle prefixed variants (system@version, stable@version)
                if let Some((prefix, version_str)) = s.split_once('@') {
                    let version = Self::parse_normalized_version(version_str)?;
                    return match prefix {
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
            ZigVersion::Unknown => {
                state.write_u8(1);
            }
        }
    }
}

impl PartialEq for ZigVersion {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            // Unknown only equals Unknown
            (Self::Unknown, Self::Unknown) => true,
            (Self::Unknown, _) | (_, Self::Unknown) => false,

            // All other variants: compare versions only
            (l, r) => match (l.version(), r.version()) {
                (Some(lv), Some(rv)) => *lv == *rv,
                (None, None) => {
                    // Both return None from version() - could be placeholder versions
                    if l.is_placeholder_version() && r.is_placeholder_version() {
                        // Compare raw versions for placeholder cases
                        let l_raw = match l {
                            ZigVersion::Semver(v)
                            | ZigVersion::Master(v)
                            | ZigVersion::Stable(v)
                            | ZigVersion::Latest(v) => Some(v),
                            ZigVersion::Unknown => None,
                        };
                        let r_raw = match r {
                            ZigVersion::Semver(v)
                            | ZigVersion::Master(v)
                            | ZigVersion::Stable(v)
                            | ZigVersion::Latest(v) => Some(v),
                            ZigVersion::Unknown => None,
                        };
                        match (l_raw, r_raw) {
                            (Some(lv), Some(rv)) => *lv == *rv,
                            _ => false,
                        }
                    } else {
                        false
                    }
                }
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
                // Handle master variant
                if let Some(master_str) = map.get("master") {
                    let version = Version::parse(master_str).map_err(de::Error::custom)?;
                    return Ok(ZigVersion::Master(version));
                }

                // Handle generic "version" key - treat as Semver
                // (Stable and Latest variants are also serialized with "version" key)
                if let Some(version_str) = map.get("version") {
                    let version = Version::parse(version_str).map_err(de::Error::custom)?;
                    return Ok(ZigVersion::Semver(version));
                }

                Err(de::Error::custom(
                    "Invalid version structure: no recognized keys found",
                ))
            }
        }
    }
}

impl fmt::Display for ZigVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZigVersion::Semver(v) => write!(f, "{}", v),
            ZigVersion::Master(v) => write!(f, "master <{}>", v),
            ZigVersion::Stable(v) => write!(f, "stable <{}>", v),
            ZigVersion::Latest(v) => write!(f, "latest <{}>", v),
            ZigVersion::Unknown => write!(f, "unknown"),
        }
    }
}

impl From<semver::Version> for ZigVersion {
    fn from(version: semver::Version) -> Self {
        ZigVersion::Semver(version)
    }
}

impl From<&semver::Version> for ZigVersion {
    fn from(version: &semver::Version) -> Self {
        ZigVersion::Semver(version.clone())
    }
}
