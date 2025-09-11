use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::ZvError;
use color_eyre::eyre::eyre;

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
        app: &crate::App,
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
            TemplateType::Embedded => self.instantiate_embedded()?,
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
    pub fn execute(mut self, app: &crate::App) -> Result<TemplateResult, ZvError> {
        let pre_exec_msg = self.prepare_directory()?;
        self.instantiate_with_context(pre_exec_msg, app)
    }

    fn instantiate_embedded(&self) -> Result<Vec<FileStatus>, ZvError> {
        let files = [
            ("main.zig", MAIN_ZIG),
            ("build.zig", BUILD_ZIG),
            (".gitignore", GITIGNORE_ZIG),
        ];

        self.create_template_files(&files)
    }

    // TODO: Add after zv 1.0.0
    // fn instantiate_minimal(&self) -> Result<Vec<FileStatus>, ZvError> {
    //     // First create embedded files
    //     let mut file_statuses = self.instantiate_embedded()?;

    //     // Then add minimal-specific files
    //     let minimal_files = [("build.zig.zon", BUILD_ZIG_ZON)];

    //     match self.create_template_files(&minimal_files) {
    //         Ok(mut minimal_statuses) => {
    //             file_statuses.append(&mut minimal_statuses);
    //             Ok(file_statuses)
    //         }
    //         Err(e) => {
    //             // Rollback the embedded files that were created
    //             self.rollback_created_files(&file_statuses);
    //             Err(e)
    //         }
    //     }
    // }

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

                if let Err(e) = write_file(&file_statuses.last().unwrap().path(), content) {
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
        if self
            .context
            .as_ref()
            .expect("Context should be initialized")
            .created_new_dir
        {
            // If we created the directory, remove the entire directory
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

    fn instantiate_zig(&self, app: &crate::App) -> Result<Vec<FileStatus>, ZvError> {
        let target_dir = &self.context.as_ref().unwrap().target_dir;

        // Get the zig path from the app
        let zig_path = app
            .zv_zig()
            .ok_or_else(|| ZvError::TemplateError(eyre!("No zig executable found")))?;

        let output = app
            .spawn_with_guard(&zig_path, &["init"], Some(target_dir))
            .map_err(|e| {
                if self.context.as_ref().unwrap().created_new_dir {
                    let _ = rda::remove_dir_all(target_dir);
                }
                e
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TemplateType {
    /// Barebones Template.
    #[default]
    Embedded,
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

pub const BUILD_ZIG_ZON: &str = r#".{
    .name = "project",
    .version = "0.0.0",
    .dependencies = .{},
    .paths = .{""},
}"#;

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
    let file_path = line.split(':').last()?.trim();

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
