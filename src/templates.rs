use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::{App, ZvError, suggest, tools};
use color_eyre::eyre::eyre;
use semver::Version;

#[derive(Debug, Clone)]
pub enum FileStatus {
    Created(PathBuf),
    Preserved(PathBuf),
}

impl FileStatus {
    pub fn path(&self) -> &Path {
        match self {
            FileStatus::Created(path) => path,
            FileStatus::Preserved(path) => path,
        }
    }

    pub fn was_created(&self) -> bool {
        matches!(self, FileStatus::Created(_))
    }

    pub fn was_preserved(&self) -> bool {
        matches!(self, FileStatus::Preserved(_))
    }
}

#[derive(Debug, Clone)]
pub struct TemplateResult {
    pub project_name: Option<String>,
    pub context: TemplateContext,
    pub file_statuses: Vec<FileStatus>,
    pub pre_exec_msg: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TemplateContext {
    /// Template dump directory
    pub target_dir: PathBuf,
    /// Did we create a new dir or are we reusing one?
    pub created_new_dir: bool,
}

impl Template {
    /// Prepare the project directory, creating it if project_name.is_some() for template instantiation.
    /// Also initializes [TemplateContext] for [Template] storing the info for the same
    pub fn prepare_directory(&mut self) -> Result<Option<String>, ZvError> {
        let mut created_new_dir = false;
        let mut pre_exec_msg: Option<String> = None;
        let target_dir = match &self.name {
            Some(name) => {
                let dir = std::env::current_dir()
                    .map_err(|e| {
                        ZvError::TemplateError(eyre!("Failed to get current directory: {}", e))
                    })?
                    .join(name);

                if dir.is_dir() {
                    // Directory exists, we'll use it as-is
                    pre_exec_msg = Some(format!("Using existing directory: {}", dir.display()));
                    dir
                } else {
                    // Try to create the directory
                    match std::fs::create_dir(&dir) {
                        Ok(()) => {
                            created_new_dir = true;
                            pre_exec_msg =
                                Some(format!("Creating new directory: {}", dir.display()));
                            dir
                        }
                        Err(err) => {
                            return Err(ZvError::TemplateError(eyre!(
                                "Failed to create project directory at {}: {}",
                                dir.display(),
                                err
                            )));
                        }
                    }
                }
            }
            None => std::env::current_dir().map_err(|e| {
                ZvError::TemplateError(eyre!("Failed to get current directory: {}", e))
            })?,
        };
        self.context = Some(TemplateContext {
            target_dir,
            created_new_dir,
        });
        Ok(pre_exec_msg)
    }

    /// Instantiate template with full context and file tracking; Needs to be called with valid context
    pub fn instantiate_with_context(
        self,
        pre_exec_msg: Option<String>,
        app: App,
    ) -> Result<TemplateResult, ZvError> {
        if !self
            .context
            .as_ref()
            .expect("Context should be initialized")
            .target_dir
            .is_dir()
        {
            return Err(ZvError::TemplateError(eyre!(
                "Directory {} not found. Aborting template instantiation",
                self.context
                    .as_ref()
                    .expect("Context should be initialized")
                    .target_dir
                    .display()
            )));
        }

        let file_statuses = match &self.r#type {
            TemplateType::App { zon } => {
                if let Some(zig_version) = app.get_active_version()
                    && let Some(v) = zig_version.version()
                {
                    let mszv = include_str!(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/templates/.zigversion"
                    ));
                    if v < &Version::parse(mszv).expect("valid .zigversion") {
                        tools::warn(
                            "Minimal build.zig template may not work with active zig version < 0.14.0",
                        );
                        suggest!(
                            "Consider using {} to initialize the project with `zig init` instead.",
                            cmd = "zv init -z"
                        );
                    }
                }
                if !zon {
                    self.instantiate_minimal()?
                } else {
                    self.instantiate_package(app)?
                }
            }
            // TemplateType::Minimal => self.instantiate_minimal()?,
            TemplateType::Zig(_zig_path) => self.instantiate_zig(app)?,
        };

        Ok(TemplateResult {
            project_name: self.name,
            context: self.context.unwrap(),
            file_statuses,
            pre_exec_msg,
        })
    }

    /// Convenience method that handles directory preparation and instantiation
    pub fn execute(mut self, app: App) -> Result<TemplateResult, ZvError> {
        let pre_exec_msg = self.prepare_directory()?;
        self.instantiate_with_context(pre_exec_msg, app)
    }

    fn instantiate_minimal(&self) -> Result<Vec<FileStatus>, ZvError> {
        let files = [
            ("main.zig", MAIN_ZIG),
            ("build.zig", BUILD_ZIG),
            (".gitignore", GITIGNORE_ZIG),
        ];

        self.create_template_files(&files)
    }

    fn instantiate_package(&self, app: App) -> Result<Vec<FileStatus>, ZvError> {
        let minimal_files = [
            ("main.zig", MAIN_ZIG),
            ("build.zig", BUILD_ZIG),
            (".gitignore", GITIGNORE_ZIG),
        ];

        let build_zig_zon = self.generate_build_zig_zon(app, &minimal_files[..2])?;

        self.create_template_files(&[
            minimal_files[0],
            minimal_files[1],
            minimal_files[2],
            ("build.zig.zon", &build_zig_zon),
        ])
    }

    /// Create template files with rollback
    fn create_template_files(&self, files: &[(&str, &str)]) -> Result<Vec<FileStatus>, ZvError> {
        let mut file_statuses = Vec::new();

        for (file_name, content) in files.iter() {
            let file_path = self
                .context
                .as_ref()
                .expect("Context should be initialized")
                .target_dir
                .join(file_name);

            if file_path.exists() {
                file_statuses.push(FileStatus::Preserved(file_path));
            } else {
                // Mark as created BEFORE attempting write, so it gets cleaned up on failure
                file_statuses.push(FileStatus::Created(file_path));

                if let Err(e) = write_file(file_statuses.last().unwrap().path(), content) {
                    // Rollback all files created in this batch (including the one that just failed)
                    self.rollback_created_files(&file_statuses);
                    return Err(e);
                }
            }
        }

        Ok(file_statuses)
    }

    /// Rollback strategy: remove created files or entire directory
    fn rollback_created_files(&self, file_statuses: &[FileStatus]) {
        // If we created the directory, remove the entire directory
        if self
            .context
            .as_ref()
            .expect("Context should be initialized")
            .created_new_dir
        {
            let _ = rda::remove_dir_all(
                &self
                    .context
                    .as_ref()
                    .expect("Context should be initialized")
                    .target_dir,
            );
        } else {
            // If we're in an existing directory, remove only the files we created
            for status in file_statuses {
                if let FileStatus::Created(path) = status {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }

    fn instantiate_zig(&self, app: App) -> Result<Vec<FileStatus>, ZvError> {
        let target_dir = &self.context.as_ref().unwrap().target_dir;

        // Get the zig path from the app
        let zig_path = app
            .zv_zig()
            .ok_or_else(|| ZvError::TemplateError(eyre!("No zig executable found")))?;

        let output = app
            .spawn_zig_with_guard(&zig_path, &["init"], Some(target_dir))
            .inspect_err(|_e| {
                if self.context.as_ref().unwrap().created_new_dir {
                    let _ = rda::remove_dir_all(target_dir);
                }
            })?;

        if !output.status.success() {
            if self.context.as_ref().unwrap().created_new_dir {
                let _ = rda::remove_dir_all(target_dir);
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ZvError::TemplateError(eyre!(
                "zig init failed with exit code {:?}: {}",
                output.status.code(),
                stderr
            )));
        }

        // Parse the output to determine which files were preserved vs created
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Zig init outputs to stderr, not stdout
        let output_text = if !stderr.is_empty() { &stderr } else { &stdout };

        let mut file_statuses = Vec::new();

        for line in output_text.lines() {
            if let Some((status, file_path)) = parse_zig_output_line(line) {
                let full_path = target_dir.join(file_path);
                file_statuses.push(status(full_path));
            }
        }

        Ok(file_statuses)
    }

    fn generate_build_zig_zon(
        &self,
        mut app: App,
        // List of path and file content pairs to be included in the build.zig.zon - we only care about the paths here
        path_files: &[(&str, &str)],
    ) -> Result<String, ZvError> {
        let active_zig_version = if let Some(active_version) = app.get_active_version() {
            active_version.version().expect("valid semver").clone()
        } else {
            // Get handle to the current runtime
            let handle = tokio::runtime::Handle::current();
            if let Ok(stable) =
                handle.block_on(app.fetch_latest_version(crate::app::CacheStrategy::OnlyCache))
            {
                tracing::debug!("Stable resolved version: {}", stable.resolved_version().version());
                stable.resolved_version().version().clone()
            } else {
                return Err(ZvError::TemplateError(eyre!(
                    "Failed to determine active Zig version for build.zig.zon generation"
                )));
            }
        };
        let mut build_zig_zon = String::with_capacity(512);
        // Determine project name. Returns None if zig version < 0.12 or if cwd is used then it returns ".app" (minor version >= 13)
        // or "app" (minor version ~ 12)
        let project_name =
            tools::sanitize_build_zig_zon_name(self.name.as_deref(), &active_zig_version);

        if active_zig_version < Version::new(0, 12, 0) {
            // build.zig.zon not supported below 0.12
            return Err(ZvError::TemplateError(
                eyre!("build.zig.zon files are only supported in Zig 0.12 and above").wrap_err(
                    format!(
                        "Cannot generate build.zig.zon with Zig version {}",
                        active_zig_version
                    ),
                ),
            ));
        }
        let name = project_name
            .expect("*zig_version < Version::new(0, 12, 0) checked during sanitization");

        build_zig_zon.push_str(".{\n");
        build_zig_zon.push_str(&format!("    .name = {name},\n"));
        build_zig_zon.push_str("    .version = \"0.0.0\",\n");

        // Generate fingerprint using the same algorithm as Zig (https://github.com/ziglang/zig/blob/60a332406c10be922568e11dcc5144bb0f2d7a85/src/Package.zig#L17-L22)
        // Fingerprint is a packed struct(u64) with:
        // - id: random u32 in range [1, 0xfffffffe]
        // - checksum: CRC32 hash of the name
        use crc32fast::Hasher;
        use rand::Rng;

        let mut rng = rand::rng();
        let id: u32 = rng.random_range(1..0xffffffff); // Excludes 0xffffffff

        // Get the name without quotes/dots for CRC32 (strip the formatting)
        let name_for_hash = name.trim_start_matches('.').trim_matches('"');
        let mut hasher = Hasher::new();
        hasher.update(name_for_hash.as_bytes());
        let checksum = hasher.finalize();

        // Pack as little-endian: id in lower 32 bits, checksum in upper 32 bits
        // This matches Zig's packed struct(u64) layout
        let fingerprint: u64 = (id as u64) | ((checksum as u64) << 32);

        build_zig_zon.push_str(&format!("    .fingerprint = 0x{fingerprint:x},\n"));

        build_zig_zon.push_str(&format!(
            "    .minimum_zig_version = \"{active_zig_version}\",\n"
        ));

        build_zig_zon.push_str("    .dependencies = .{},\n");
        build_zig_zon.push_str("    .paths = .{\n");
        for (path, _) in path_files {
            build_zig_zon.push_str(&format!("        \"{path}\",\n"));
        }
        build_zig_zon.push_str("    },\n");
        build_zig_zon.push_str("}\n");
        Ok(build_zig_zon)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Template {
    name: Option<String>,
    context: Option<TemplateContext>,
    r#type: TemplateType,
}

impl Template {
    pub fn new(name: Option<String>, r#type: TemplateType) -> Self {
        Self {
            name,
            context: None,
            r#type,
        }
    }
}

impl Default for TemplateType {
    fn default() -> Self {
        TemplateType::App { zon: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateType {
    /// Barebones Template.
    App { zon: bool },
    /// Library Template with src/root.zig, unit test, and build.zig.zon (optional)
    // Library { zon: bool }, //TODO: unimplemented
    /// Minimal Template with build.zig.zon & unit test
    // Minimal, //TODO: unimplemented
    /// Template initialized using Zig
    Zig(PathBuf), // inner points to zig exe to use for zig init
}

pub const GITIGNORE_ZIG: &str = r#"zig-out
.zig-cache"#;

pub const MAIN_ZIG: &str = r#"pub fn main() !void {
    std.log.info("Hello, World!", .{});
}
    
const std = @import("std");"#;

pub const BUILD_ZIG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/lean_build.zig"
));

fn write_file(path: &Path, content: &str) -> Result<(), ZvError> {
    fs::File::create(path)
        .map_err(|e| ZvError::TemplateError(eyre!("Failed to create {}: {}", path.display(), e)))?
        .write_all(content.as_bytes())
        .map_err(|e| {
            ZvError::TemplateError(eyre!("Failed to write to {}: {}", path.display(), e))
        })?;
    Ok(())
}

/// Parse a line from zig init output to extract file operations
/// Returns (status_constructor, file_path) if a file operation is found
fn parse_zig_output_line(line: &str) -> Option<(fn(PathBuf) -> FileStatus, &str)> {
    let line = line.trim();

    // Find the last colon and extract the file path after it
    let file_path = line.split(':').next_back()?.trim();

    // Skip empty paths or lines that don't look like file paths
    if file_path.is_empty()
        || (!file_path.contains('.') && !file_path.contains('/') && !file_path.contains('\\'))
    {
        return None;
    }

    // Determine the operation based on keywords in the line
    if line.contains("preserving") || line.contains("preserved") {
        Some((FileStatus::Preserved, file_path))
    } else if line.contains("created") {
        Some((FileStatus::Created, file_path))
    } else {
        None
    }
}
