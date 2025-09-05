use crate::{App, Shell, UserConfig, ZigVersion, ZvError, tools};
use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context as _, eyre};
use yansi::Paint;
mod init;
mod list;
mod sync;
mod r#use;
mod zig;
mod zls;

pub use zig::zig_main;
pub use zls::zls_main;

pub async fn zv_main() -> super::Result<()> {
    let zv_cli = <ZvCli as clap::Parser>::parse();
    let (zv_dir, using_env) = tools::fetch_zv_dir()?;
    if using_env {
        tracing::info!("Using ZV_DIR from environment: {}", zv_dir.display());
    }
    // TODO: Allow force flags to skip prompts in ZvCli
    // let allow_shell = zv_cli.allow_shell || zv_cli.force;
    // let force = zv_cli.force;
    // let g = Genie { allow_shell, force };

    let app = App::init(UserConfig {
        path: zv_dir,
        shell: Shell::detect(),
    })?;

    match zv_cli.command {
        Some(cmd) => cmd.execute(app).await?,
        None => {
            println!("~ ZV ~");
            println!("{}", tools::sys_info());
        }
    }
    Ok(())
}

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
                    return init::init_project(
                        Template::new(
                            project_name,
                            TemplateType::Zig(
                                app.zv_zig_or_system()
                                    .ok_or_else(|| eyre!("No Zig executable found"))?,
                            ),
                        ),
                        &app,
                    );
                } else {
                    init::init_project(Template::new(project_name, TemplateType::Embedded), &app)
                }
            }
            Commands::Use { version, path } => {
                match (version, path) {
                    (Some(version), None) => r#use::use_version(version, &mut app).await,
                    (None, Some(_path)) => {
                        eprintln!(
                            "{}",
                            Paint::red("Error: --path option is no longer supported. System Zig handling has been simplified.")
                        );
                        std::process::exit(1);
                    }
                    (None, None) => {
                        eprintln!("{}", Paint::red("Error: Version must be specified"));
                        std::process::exit(1);
                    }
                    (Some(_version), Some(_path)) => {
                        eprintln!(
                            "{}",
                            Paint::red("Error: --path option is no longer supported. System Zig handling has been simplified.")
                        );
                        std::process::exit(1);
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
