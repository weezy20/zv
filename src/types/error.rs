use color_eyre::Report;

#[derive(thiserror::Error, Debug)]
/// ZV error type
pub enum ZvError {
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

    /// SystemZig Error
    #[error("SystemZig error")]
    SystemZigError(#[source] Report),

    /// Zig Error
    #[error("Zig error")]
    ZigError(#[source] Report),

    /// Catch-all for general errors
    #[error(transparent)]
    General(#[from] Report),
}

#[derive(thiserror::Error, Debug)]
pub enum NetErr {
    #[error("Network IO error: {0}")]
    FileIo(#[source] std::io::Error),

    #[error("Reqwest error: {0}")]
    Network(#[source] reqwest::Error),

    #[error("HTTP request failed with status: {0}")]
    HttpStatus(reqwest::StatusCode),

    #[error("JSON parse error: {0}")]
    JsonParse(#[source] serde_json::Error),

    #[error("JSON serialize error: {0}")]
    JsonSerialize(#[source] serde_json::Error),

    #[error("TOML parse error: {0}")]
    TomlParse(#[source] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSerialize(#[source] toml::ser::Error),

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
    #[error("Config flush failed")]
    WriteFail(#[source] Report),
}
