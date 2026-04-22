pub const ZIG_DOWNLOAD_INDEX_JSON: &str = "https://ziglang.org/download/index.json";

pub const ZIG_COMMUNITY_MIRRORS: &str = "https://ziglang.org/download/community-mirrors.txt";

/// Not expected to change unless some catastrophe at which point this should be updated
pub const ZIG_MINSIGN_PUBKEY: &str = r#"RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U"#;

/// minisign public key used to verify ZLS prebuilt artifacts.
pub const ZLS_MINISIGN_PUBKEY: &str = r#"RWR+9B91GBZ0zOjh6Lr17+zKf5BoSuFvrx2xSeDE57uIYvnKBGmMjOex"#;

/// Zigtools API endpoint for selecting ZLS compatible with an active Zig.
pub const ZLS_SELECT_VERSION_ENDPOINT: &str = "https://releases.zigtools.org/v1/zls/select-version";

/// Zv's knowledge of what the current master semver is
pub const ZV_MASTER_FILE: &str = "master";
