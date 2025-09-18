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
    Master(Option<Version>),
    /// Latest stable (cached)
    Stable(Option<Version>),
    /// Latest stable (always refresh)
    Latest(Option<Version>),
}

impl ZigVersion {
    /// Creates a placeholder version (None) for the given variant type
    pub fn placeholder_for_variant(variant: &str) -> Result<Self, ZvError> {
        match variant {
            "master" => Ok(ZigVersion::Master(None)),
            "stable" => Ok(ZigVersion::Stable(None)),
            "latest" => Ok(ZigVersion::Latest(None)),
            _ => Err(ZvError::General(eyre!("Invalid variant: {}", variant))),
        }
    }

    /// Normalizes a version string to semver format (e.g., "1" -> "1.0.0", "1.2" -> "1.2.0")
    fn parse_normalized_version(version_str: &str) -> Result<Version, ZvError> {
        // First, separate the core version from pre-release and build metadata
        let (core_version, suffix) = if let Some(hyphen_pos) = version_str.find('-') {
            (&version_str[..hyphen_pos], &version_str[hyphen_pos..])
        } else if let Some(plus_pos) = version_str.find('+') {
            (&version_str[..plus_pos], &version_str[plus_pos..])
        } else {
            (version_str, "")
        };

        // Normalize only the core version part (before any - or +)
        let normalized_core = match core_version.chars().filter(|&c| c == '.').count() {
            0 => format!("{}.0.0", core_version),
            1 => format!("{}.0", core_version),
            _ => core_version.to_string(),
        };

        // Combine normalized core with original suffix
        let normalized = format!("{}{}", normalized_core, suffix);

        Version::parse(&normalized).map_err(ZvError::ZigVersionError)
    }

    /// Extracts the version from any ZigVersion variant, if available
    pub fn version(&self) -> Option<&Version> {
        match self {
            ZigVersion::Semver(v) => Some(v),
            ZigVersion::Master(Some(v))
            | ZigVersion::Stable(Some(v))
            | ZigVersion::Latest(Some(v)) => Some(v),
            ZigVersion::Master(None) | ZigVersion::Stable(None) | ZigVersion::Latest(None) => None,
        }
    }

    /// Returns true if the version has a concrete version
    pub fn contains_semver(&self) -> bool {
        match self {
            ZigVersion::Semver(_) => true,
            ZigVersion::Master(Some(_))
            | ZigVersion::Stable(Some(_))
            | ZigVersion::Latest(Some(_)) => true,
            ZigVersion::Master(None) | ZigVersion::Stable(None) | ZigVersion::Latest(None) => false,
        }
    }

    /// Returns the variant type as a string
    pub fn variant_type(&self) -> &'static str {
        match self {
            ZigVersion::Semver(_) => "semver",
            ZigVersion::Master(_) => "master",
            ZigVersion::Stable(_) => "stable",
            ZigVersion::Latest(_) => "latest",
        }
    }
}

impl FromStr for ZigVersion {
    type Err = ZvError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "master" => Self::placeholder_for_variant("master"),
            "stable" => Self::placeholder_for_variant("stable"),
            "latest" => Self::placeholder_for_variant("latest"),
            _ => {
                // Handle prefixed variants (stable@version)
                if let Some((prefix, version_str)) = s.split_once('@') {
                    let version = Self::parse_normalized_version(version_str)?;
                    return match prefix {
                        "stable" => {
                            // Validate that the version is stable (no pre-release or dev builds)
                            if version.pre.is_empty() && version.build.is_empty() {
                                Ok(ZigVersion::Stable(Some(version)))
                            } else {
                                Err(ZvError::General(eyre!(
                                    "stable@<version> only accepts stable versions. '{}' appears to be a pre-release or dev build",
                                    version_str
                                )))
                            }
                        }
                        "master" => Ok(ZigVersion::Master(Some(version))),
                        "latest" => Ok(ZigVersion::Latest(Some(version))),
                        _ => Err(ZvError::General(eyre!(
                            "Invalid version prefix: {}. Supported: stable@<version>",
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
            ZigVersion::Semver(v) => {
                state.write_u8(0);
                v.hash(state);
            }
            ZigVersion::Master(v) => {
                state.write_u8(1);
                v.hash(state);
            }
            ZigVersion::Stable(v) => {
                state.write_u8(2);
                v.hash(state);
            }
            ZigVersion::Latest(v) => {
                state.write_u8(3);
                v.hash(state);
            }
        }
    }
}

impl PartialEq for ZigVersion {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ZigVersion::Semver(a), ZigVersion::Semver(b)) => a == b,
            (ZigVersion::Master(a), ZigVersion::Master(b)) => a == b,
            (ZigVersion::Stable(a), ZigVersion::Stable(b)) => a == b,
            (ZigVersion::Latest(a), ZigVersion::Latest(b)) => a == b,
            // Different variant types are never equal
            _ => false,
        }
    }
}

impl Eq for ZigVersion {}

impl Serialize for ZigVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let version_str = match self {
            ZigVersion::Semver(version) => version.to_string(),
            ZigVersion::Master(Some(version))
            | ZigVersion::Stable(Some(version))
            | ZigVersion::Latest(Some(version)) => version.to_string(),
            ZigVersion::Master(None) | ZigVersion::Stable(None) | ZigVersion::Latest(None) => {
                return Err(serde::ser::Error::custom(
                    "Cannot serialize unresolved version",
                ));
            }
        };

        serializer.serialize_str(&version_str)
    }
}

impl<'de> Deserialize<'de> for ZigVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let version_str = String::deserialize(deserializer)?;
        ZigVersion::from_str(&version_str).map_err(de::Error::custom)
    }
}

impl fmt::Display for ZigVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZigVersion::Semver(v) => write!(f, "{}", v),
            ZigVersion::Master(Some(v)) => write!(f, "master <{}>", v),
            ZigVersion::Master(None) => write!(f, "master <version: unknown>"),
            ZigVersion::Stable(Some(v)) => write!(f, "stable <{}>", v),
            ZigVersion::Stable(None) => write!(f, "stable <version: unknown>"),
            ZigVersion::Latest(Some(v)) => write!(f, "latest <{}>", v),
            ZigVersion::Latest(None) => write!(f, "latest <version: unknown>"),
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
