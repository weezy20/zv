//! Target triple representation for Zig platforms

use std::hash::{Hash, Hasher};

/// Type-safe representation of a target triple (architecture-operating system)
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TargetTriple {
    pub arch: String,
    pub os: String,
}

impl TargetTriple {
    /// Create a new TargetTriple with the given architecture and operating system
    pub fn new(arch: String, os: String) -> Self {
        Self { arch, os }
    }

    /// Parse a target key string in "arch-os" format into a TargetTriple
    ///
    /// # Arguments
    /// * `key` - A string in the format "arch-os" (e.g., "x86_64-linux")
    ///
    /// # Returns
    /// * `Some(TargetTriple)` if the key can be parsed successfully
    /// * `None` if the key format is invalid
    ///
    /// # Examples
    /// ```
    /// use zv::types::TargetTriple;
    ///
    /// let triple = TargetTriple::from_key("x86_64-linux").unwrap();
    /// assert_eq!(triple.arch, "x86_64");
    /// assert_eq!(triple.os, "linux");
    /// ```
    pub fn from_key(key: &str) -> Option<Self> {
        let parts: Vec<&str> = key.split('-').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            Some(Self::new(parts[0].to_string(), parts[1].to_string()))
        } else {
            None
        }
    }

    /// Generate a target key string in "arch-os" format
    ///
    /// # Returns
    /// A string in the format "arch-os"
    ///
    /// # Examples
    /// ```
    /// use zv::types::TargetTriple;
    ///
    /// let triple = TargetTriple::new("x86_64".to_string(), "linux".to_string());
    /// assert_eq!(triple.to_key(), "x86_64-linux");
    /// ```
    pub fn to_key(&self) -> String {
        format!("{}-{}", self.arch, self.os)
    }
}

impl Hash for TargetTriple {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.arch.hash(state);
        self.os.hash(state);
    }
}
