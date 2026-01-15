use color_eyre::Report;

#[derive(thiserror::Error, Debug)]
pub enum ShellErr {
    #[error("Setup failed in {phase}: {reason}")]
    SetupFailed { phase: String, reason: String },

    #[error("Pre-setup check failed: {check}")]
    PreSetupCheckFailed { check: String },

    #[error("Environment file operation failed: {operation} on {file_path}")]
    EnvironmentFileFailed {
        operation: String,
        file_path: String,
    },

    #[error("Windows registry operation failed: {operation}")]
    RegistryFailed { operation: String },

    #[error("Failed to modify shell RC file: {file_path}")]
    RcFileModificationFailed {
        file_path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("User declined confirmation for: {operation}")]
    UserDeclinedConfirmation { operation: String },

    #[error("Failed to modify PATH: {reason}")]
    PathModificationFailed { reason: String },

    #[error("Failed to detect shell environment: {details}")]
    DetectionFailed { details: String },

    #[error("Shell setup context creation failed: {reason}")]
    ContextCreationFailed { reason: String },

    #[error("ZV_DIR operation failed: {operation}")]
    ZvDirOperationFailed { operation: String },

    #[error("PATH operation failed: {operation}")]
    PathOperationFailed { operation: String },

    #[error("Post-setup action failed: {action}")]
    PostSetupActionFailed { action: String },

    #[error("Shell environment validation failed: {validation}")]
    ValidationFailed { validation: String },

    #[error("Unsupported shell configuration: {shell_type} on {platform}")]
    UnsupportedConfiguration {
        shell_type: String,
        platform: String,
    },

    #[error("Interactive prompt failed: {reason}")]
    InteractivePromptFailed { reason: String },

    #[error("Interactive mode not available: {reason}")]
    InteractiveModeNotAvailable { reason: String },

    #[error("User cancelled interactive setup")]
    UserCancelledInteractive,
}

impl ShellErr {
    /// Create a setup failed error with phase context
    pub fn setup_failed(phase: &str, reason: &str) -> Self {
        Self::SetupFailed {
            phase: phase.to_string(),
            reason: reason.to_string(),
        }
    }

    /// Create a pre-setup check failed error
    pub fn pre_setup_check_failed(check: &str) -> Self {
        Self::PreSetupCheckFailed {
            check: check.to_string(),
        }
    }

    /// Create an environment file operation failed error
    pub fn environment_file_failed(operation: &str, file_path: &str) -> Self {
        Self::EnvironmentFileFailed {
            operation: operation.to_string(),
            file_path: file_path.to_string(),
        }
    }

    /// Create a registry operation failed error
    pub fn registry_failed(operation: &str) -> Self {
        Self::RegistryFailed {
            operation: operation.to_string(),
        }
    }

    /// Create an RC file modification failed error
    pub fn rc_file_modification_failed(file_path: &str, source: std::io::Error) -> Self {
        Self::RcFileModificationFailed {
            file_path: file_path.to_string(),
            source,
        }
    }

    /// Create a user declined confirmation error
    pub fn user_declined_confirmation(operation: &str) -> Self {
        Self::UserDeclinedConfirmation {
            operation: operation.to_string(),
        }
    }

    /// Create a PATH modification failed error
    pub fn path_modification_failed(reason: &str) -> Self {
        Self::PathModificationFailed {
            reason: reason.to_string(),
        }
    }

    /// Create a detection failed error
    pub fn detection_failed(details: &str) -> Self {
        Self::DetectionFailed {
            details: details.to_string(),
        }
    }

    /// Create a context creation failed error
    pub fn context_creation_failed(reason: &str) -> Self {
        Self::ContextCreationFailed {
            reason: reason.to_string(),
        }
    }

    /// Create a ZV_DIR operation failed error
    pub fn zv_dir_operation_failed(operation: &str) -> Self {
        Self::ZvDirOperationFailed {
            operation: operation.to_string(),
        }
    }

    /// Create a PATH operation failed error
    pub fn path_operation_failed(operation: &str) -> Self {
        Self::PathOperationFailed {
            operation: operation.to_string(),
        }
    }

    /// Create a post-setup action failed error
    pub fn post_setup_action_failed(action: &str) -> Self {
        Self::PostSetupActionFailed {
            action: action.to_string(),
        }
    }

    /// Create a validation failed error
    pub fn validation_failed(validation: &str) -> Self {
        Self::ValidationFailed {
            validation: validation.to_string(),
        }
    }

    /// Create an unsupported configuration error
    pub fn unsupported_configuration(shell_type: &str, platform: &str) -> Self {
        Self::UnsupportedConfiguration {
            shell_type: shell_type.to_string(),
            platform: platform.to_string(),
        }
    }

    /// Create an interactive prompt failed error
    pub fn interactive_prompt_failed(reason: &str) -> Self {
        Self::InteractivePromptFailed {
            reason: reason.to_string(),
        }
    }

    /// Create an interactive mode not available error
    pub fn interactive_mode_not_available(reason: &str) -> Self {
        Self::InteractiveModeNotAvailable {
            reason: reason.to_string(),
        }
    }

    /// Create a user cancelled interactive error
    pub fn user_cancelled_interactive() -> Self {
        Self::UserCancelledInteractive
    }

    /// Get error recovery suggestions for common failure modes
    pub fn recovery_suggestion(&self) -> Option<String> {
        match self {
            Self::RegistryFailed { operation } => {
                Some(format!(
                    "Registry operation '{}' failed. Try running as administrator or check Windows permissions.",
                    operation
                ))
            }
            Self::RcFileModificationFailed { file_path, .. } => {
                Some(format!(
                    "Failed to modify RC file '{}'. Check file permissions and ensure the directory exists.",
                    file_path
                ))
            }
            Self::EnvironmentFileFailed { operation, file_path } => {
                Some(format!(
                    "Environment file operation '{}' failed on '{}'. Check file permissions and disk space.",
                    operation, file_path
                ))
            }
            Self::PathModificationFailed { reason } => {
                Some(format!(
                    "PATH modification failed: {}. Try manually adding the zv bin directory to your PATH.",
                    reason
                ))
            }
            Self::DetectionFailed { .. } => {
                Some("Shell detection failed. Try setting the SHELL environment variable or use a supported shell.".to_string())
            }
            Self::UnsupportedConfiguration { shell_type, platform } => {
                Some(format!(
                    "Shell '{}' on '{}' is not supported. Try using a standard shell like bash, zsh, or PowerShell.",
                    shell_type, platform
                ))
            }
            Self::UserDeclinedConfirmation { operation } => {
                Some(format!(
                    "Setup incomplete because '{}' was declined. You can run 'zv setup' again to retry.",
                    operation
                ))
            }
            Self::InteractivePromptFailed { reason } => {
                Some(format!(
                    "Interactive prompt failed: {}. Try running 'zv setup --no-interactive' to use non-interactive mode.",
                    reason
                ))
            }
            Self::InteractiveModeNotAvailable { reason } => {
                Some(format!(
                    "Interactive mode not available: {}. Falling back to non-interactive mode.",
                    reason
                ))
            }
            Self::UserCancelledInteractive => {
                Some("Setup was cancelled by user. You can run 'zv setup' again to retry.".to_string())
            }
            _ => None,
        }
    }
}

#[derive(thiserror::Error, Debug)]
/// ZV error type
pub enum ZvError {
    /// Cannot Set Active Version
    #[error("Cannot set active version to {version}: {reason}")]
    CannotSetActiveVersion {
        version: semver::Version,
        reason: String,
    },
    /// Cache not found at location
    #[error("File not found at: {0}")]
    CacheNotFound(String),

    /// Failure type for parse Zig version
    #[error("failed to parse semantic version")]
    ZigVersionError(#[from] semver::Error),

    /// Failure to resolve Zig Version
    #[error("Failed to resolve Zig version")]
    ZigVersionResolveError(#[source] Report),

    /// ZvState init failed
    #[error("Failed to initialize App")]
    ZvAppInitError(#[source] Report),

    /// Io related errors
    #[error("I/O Error")]
    Io(#[source] std::io::Error),

    /// Zv config is invalid
    #[error("ZvConfig Error")]
    ZvConfigError(#[source] CfgErr),

    /// Zv bin path doesn't exist
    #[error("Zv bin path not found")]
    ZvBinPathNotFound,

    /// Zv Export Failed
    #[error("Failed to export environment")]
    ZvExportError(#[source] Report),

    /// Zig Execute failed
    #[error("Zig cmd failure: {command}")]
    ZigExecuteError {
        command: String,
        #[source]
        source: Report,
    },

    /// Template Error
    #[error("Template error")]
    TemplateError(#[source] Report),

    /// Network Error
    #[error("Network error")]
    NetworkError(#[source] NetErr),

    /// Zig Error
    #[error("Zig not found")]
    ZigNotFound(#[source] Report),

    /// Shell setup and environment errors
    #[error("Shell error")]
    ShellError(#[from] ShellErr),

    /// Minisign signature verification failed
    #[error("Minisign error")]
    MinisignError(#[source] Report),

    /// Catch-all for general errors
    #[error(transparent)]
    General(#[from] Report),
}

impl ZvError {
    /// Create a shell setup error with phase context
    pub fn shell_setup_failed(phase: &str, reason: &str) -> Self {
        Self::ShellError(ShellErr::setup_failed(phase, reason))
    }

    /// Create a shell pre-setup check error
    pub fn shell_pre_setup_check_failed(check: &str) -> Self {
        Self::ShellError(ShellErr::pre_setup_check_failed(check))
    }

    /// Create a shell environment file error
    pub fn shell_environment_file_failed(operation: &str, file_path: &str) -> Self {
        Self::ShellError(ShellErr::environment_file_failed(operation, file_path))
    }

    /// Create a shell registry error
    pub fn shell_registry_failed(operation: &str) -> Self {
        Self::ShellError(ShellErr::registry_failed(operation))
    }

    /// Create a shell RC file modification error
    pub fn shell_rc_file_modification_failed(file_path: &str, source: std::io::Error) -> Self {
        Self::ShellError(ShellErr::rc_file_modification_failed(file_path, source))
    }

    /// Create a shell user declined confirmation error
    pub fn shell_user_declined_confirmation(operation: &str) -> Self {
        Self::ShellError(ShellErr::user_declined_confirmation(operation))
    }

    /// Create a shell PATH modification error
    pub fn shell_path_modification_failed(reason: &str) -> Self {
        Self::ShellError(ShellErr::path_modification_failed(reason))
    }

    /// Create a shell detection error
    pub fn shell_detection_failed(details: &str) -> Self {
        Self::ShellError(ShellErr::detection_failed(details))
    }

    /// Create a shell context creation error
    pub fn shell_context_creation_failed(reason: &str) -> Self {
        Self::ShellError(ShellErr::context_creation_failed(reason))
    }

    /// Create a shell ZV_DIR operation error
    pub fn shell_zv_dir_operation_failed(operation: &str) -> Self {
        Self::ShellError(ShellErr::zv_dir_operation_failed(operation))
    }

    /// Create a shell PATH operation error
    pub fn shell_path_operation_failed(operation: &str) -> Self {
        Self::ShellError(ShellErr::path_operation_failed(operation))
    }

    /// Create a shell post-setup action error
    pub fn shell_post_setup_action_failed(action: &str) -> Self {
        Self::ShellError(ShellErr::post_setup_action_failed(action))
    }

    /// Create a shell validation error
    pub fn shell_validation_failed(validation: &str) -> Self {
        Self::ShellError(ShellErr::validation_failed(validation))
    }

    /// Create a shell unsupported configuration error
    pub fn shell_unsupported_configuration(shell_type: &str, platform: &str) -> Self {
        Self::ShellError(ShellErr::unsupported_configuration(shell_type, platform))
    }

    /// Create a shell interactive prompt failed error
    pub fn shell_interactive_prompt_failed(reason: &str) -> Self {
        Self::ShellError(ShellErr::interactive_prompt_failed(reason))
    }

    /// Create a shell interactive mode not available error
    pub fn shell_interactive_mode_not_available(reason: &str) -> Self {
        Self::ShellError(ShellErr::interactive_mode_not_available(reason))
    }

    /// Create a shell user cancelled interactive error
    pub fn shell_user_cancelled_interactive() -> Self {
        Self::ShellError(ShellErr::user_cancelled_interactive())
    }

    /// Get error recovery suggestions if available
    pub fn recovery_suggestion(&self) -> Option<String> {
        match self {
            Self::ShellError(shell_err) => shell_err.recovery_suggestion(),
            Self::ZvBinPathNotFound => {
                Some("The zv bin directory was not found. Try reinstalling zv or check your installation.".to_string())
            }
            Self::ZvExportError(_) => {
                Some("Environment export failed. Check your shell configuration and permissions.".to_string())
            }
            Self::NetworkError(_) => {
                Some("Network operation failed. Check your internet connection and try again.".to_string())
            }
            _ => None,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum NetErr {
    #[error("Invalid Mirror: {0}")]
    InvalidMirror(#[source] Report),

    #[error("No valid mirrors found")]
    EmptyMirrors,

    #[error("Network IO error: {0}")]
    FileIo(#[source] std::io::Error),

    #[error("Reqwest error")]
    Reqwest(#[source] reqwest::Error),

    #[error("Download timeout: {0}")]
    Timeout(String),

    #[error("Download stalled: no progress for {duration:?}")]
    Stalled { duration: std::time::Duration },

    #[error("Too many retries: {attempts} attempts failed")]
    TooManyRetries { attempts: usize },

    #[error("HTTP request failed with status: {0}")]
    HTTP(reqwest::StatusCode),

    #[error("JSON parse error: {0}")]
    JsonParse(#[source] serde_json::Error),

    #[error("JSON serialize error: {0}")]
    JsonSerialize(#[source] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[source] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[source] toml::ser::Error),

    #[error("Failed to save cache: {0}")]
    CacheSaveFail(String),

    #[error("Mirror not found: {0}")]
    MirrorNotFound(String),

    #[error("Checksum verification failed")]
    Checksum(#[source] Report),

    #[error(transparent)]
    Other(#[from] Report),
}

#[derive(thiserror::Error, Debug)]
/// Zv config error type
pub enum CfgErr {
    /// Failure to read config file
    #[error("Config file not found or unreadable")]
    NotFound(#[source] Report),

    /// Failure to parse config file
    #[error("Config file contains invalid TOML")]
    ParseFail(#[source] Report),

    /// Failure to serialize
    #[error("Serialize failed")]
    SerializeFail(#[source] toml::ser::Error),

    /// Write failed
    #[error("Config flush failed for file: {1}")]
    WriteFail(#[source] Report, String),

    /// Cache expired
    #[error("Cache expired for {0}")]
    CacheExpired(String),
}
