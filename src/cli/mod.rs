use crate::{App, ZigVersion};
use clap::{Parser, Subcommand};
use color_eyre::eyre::eyre;
use yansi::Paint;

mod init;
mod list;
mod sync;
mod r#use;

/// zv - Zig Version (zv) Manager
///
/// Download, install, and manage Zig versions
#[derive(Parser, Debug)]
#[command(name = "zv")]
#[command(
    author,
    version,
    about = "zv - A Zig Version Manager",
    long_about = "A fast, easy to use, Zig programming language installer and version manager. \
    To find out more, run `-h` or `--help` with the subcommand you're interested in. Example: `zv install -h` for short help \
    or `zv install --help` for long help."
)]
pub struct ZvCli {
    /// Global options
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new Zig project from template
    Init {
        /// Name of the project. If none is provided zv init creates it in the current working directory.
        project_name: Option<String>,
        /// Use `zig init` instead to create a new Zig project
        #[arg(
            long = "zig",
            short = 'z',
            help = "Use `zig init` instead to create a new Zig project"
        )]
        zig: bool,
    },

    /// Select which Zig version to use
    Use {
        /// Version of Zig to use
        #[arg(
            value_parser = clap::value_parser!(ZigVersion),
            help = "The version of Zig to use. Use 'master', 'system@<version>', 'stable@<version>', 'stable', or a semantic version (e.g., '0.15.1')",
            long_help = "The version of Zig to use. Options:\n\
                         • master             - Use master branch build\n\
                         • <semver>           - Use specific version (e.g., 0.13.0, 1.2.3)\n\
                         • system@<version>   - Use system (non-zv) version (e.g., system@0.14.0)\n\
                         • stable@<version>   - Use specific stable version. Identical to just <version> (e.g., stable@0.13.0)\n\
                         • stable             - Use latest stable release\n\
                         • latest             - Use latest stable release (queries network instead of relying on cached index)"
        )]
        version: Option<ZigVersion>,

        /// Use system Zig at specific path
        #[arg(
            short = 'p',
            long = "path",
            help = "Path to system Zig executable (only allowed with system versions)",
            long_help = "Explicit path to a system-installed (non-zv managed) Zig executable.\n\
                         The path should point to the zig binary (e.g., /usr/bin/zig, C:\\zig\\zig.exe).\n\
                         Can only be used with 'system' or 'system@<version>' arguments."
        )]
        path: Option<std::path::PathBuf>,
    },

    /// List installed Zig versions (including system-wide installations if found in $PATH)
    #[clap(name = "list", alias = "ls")]
    List,

    /// Clean up Zig installations. Non-zv managed installations will not be affected.
    #[clap(name = "clean", alias = "rm")]
    Clean { version: Option<ZigVersion> },

    /// Setup shell environment for zv (required to make zig binaries available in $PATH)
    Setup,

    /// Synchronize index, mirrors list and metadata for zv. Also re-scans PATH to resync information about non-zv managed installations.
    Sync,
}

impl Commands {
    pub(crate) async fn execute(self, mut app: App) -> super::Result<()> {
        match self {
            Commands::Init { project_name, zig } => {
                use crate::{Template, TemplateType};
                if zig {
                    return init::init_project(Template::new(
                        project_name,
                        TemplateType::Zig(
                            app.zv_zig_or_system()
                                .ok_or_else(|| eyre!("No Zig executable found"))?,
                        ),
                    ));
                } else {
                    init::init_project(Template::new(project_name, TemplateType::Embedded))
                }
            }
            Commands::Use { version, path } => {
                match (version, path) {
                    (Some(version), None) => r#use::use_version(version, &mut app).await,
                    (None, Some(path)) => {
                        // --path without version means use system Zig at that path
                        let system_version = ZigVersion::System {
                            version: None,
                            path: Some(path),
                        };
                        r#use::use_version(system_version, &mut app).await
                    }
                    (None, None) => {
                        eprintln!("{}", Paint::red("Error: Version must be specified"));
                        std::process::exit(1);
                    }
                    (Some(version), Some(path)) => {
                        // Only allow --path with System variants
                        match &version {
                            ZigVersion::System { .. } => {
                                // Create a new System version with the provided path
                                let system_version_with_path = match &version {
                                    ZigVersion::System { version: v, .. } => ZigVersion::System {
                                        version: v.clone(),
                                        path: Some(path),
                                    },
                                    _ => unreachable!(), // We already matched System above
                                };
                                r#use::use_version(system_version_with_path, &mut app).await
                            }
                            _ => {
                                eprintln!(
                                    "{}",
                                    Paint::red(
                                        "Error: --path can only be used with system versions (e.g., 'system' or 'system@0.14.0')"
                                    )
                                );
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }
            Commands::List => todo!(),
            Commands::Clean { version: _version } => todo!(),
            Commands::Setup => todo!(),
            Commands::Sync => todo!(),
        }
    }
}
