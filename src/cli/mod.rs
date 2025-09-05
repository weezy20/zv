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
    let (zv_base_path, using_env) = tools::fetch_zv_dir()?;
    if using_env {
        tracing::debug!("Using ZV_DIR from environment: {}", zv_base_path.display());
    }
    // TODO: Allow force flags to skip prompts in ZvCli
    // let allow_shell = zv_cli.allow_shell || zv_cli.force;
    // let force = zv_cli.force;
    // let g = Genie { allow_shell, force };

    let app = App::init(UserConfig {
        zv_base_path,
        shell: Some(Shell::detect()),
    })?;

    match zv_cli.command {
        Some(cmd) => cmd.execute(app).await?,
        None => {
            print_welcome_message(app);
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
    /// Initialize a new Zig project from lean or standard zig template
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

    /// Select which Zig version to use - master | latest | stable | <semver>,
    Use {
        /// Version of Zig to use
        #[arg(
            value_parser = clap::value_parser!(ZigVersion),
            help = "The version of Zig to use. Use 'master', 'stable@<version>', 'stable', 'latest', or simply <version> (e.g., '0.15.1')",
            long_help = "The version of Zig to use. Options:\n\
                         • master             - Use master branch build\n\
                         • <semver>           - Use specific version (e.g., 0.13.0, 1.2.3)\n\
                         • stable@<version>   - Use specific stable version. Identical to just <version> (e.g., stable@0.13.0)\n\
                         • stable             - Use latest stable release\n\
                         • latest             - Use latest stable release (queries network instead of relying on cached index)"
        )]
        version: Option<ZigVersion>,
    },

    /// List installed Zig versions
    #[clap(name = "list", alias = "ls")]
    List,

    /// Clean up Zig installations. Non-zv managed installations will not be affected.
    #[clap(name = "clean", alias = "rm")]
    Clean { version: Option<ZigVersion> },

    /// Setup shell environment for zv (required to make zig binaries available in $PATH)
    Setup,

    /// Synchronize index, mirrors list and metadata for zv.
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
                                app.zv_zig()
                                    .ok_or_else(|| eyre!("No Zig executable found"))?,
                            ),
                        ),
                        &app,
                    );
                } else {
                    init::init_project(Template::new(project_name, TemplateType::Embedded), &app)
                }
            }
            Commands::Use { version } => match version {
                Some(version) => r#use::use_version(version, &mut app).await,
                None => {
                    eprintln!("{}", Paint::red("Error: Version must be specified"));
                    std::process::exit(1);
                }
            },
            Commands::List => todo!(),
            Commands::Clean { version: _version } => todo!(),
            Commands::Setup => todo!(),
            Commands::Sync => todo!(),
        }
    }
}

fn print_welcome_message(app: App) {
    use target_lexicon::HOST;

    // Parse the target triplet (format: arch-platform-os)
    let architecture = HOST.architecture;
    let platform = HOST.vendor;
    let os = HOST.operating_system;

    // Get shell information
    let shell = if cfg!(windows) {
        "PowerShell"
    } else {
        // You might want to detect the actual shell on Unix systems
        "Bash"
    };

    // ASCII art for ZV
    println!(
        "{}",
        Paint::yellow(&format!(
            r#"
███████╗██╗   ██╗    Architecture: {architecture}
╚══███╔╝██║   ██║    Platform: {platform}
  ███╔╝ ██║   ██║    OS: {os}
 ███╔╝  ██║   ██║    ZV directory: {zv_dir}
███████╗╚██████╔╝    Shell: {shell}
╚══════╝ ╚═════╝     Profile: {profile}
    "#,
            zv_dir = app.path().display(),
            shell = app.shell.as_ref().map_or(Shell::detect(), |s| *s),
            profile = std::env::var("PROFILE").unwrap_or_else(|_| "Not set".to_string())
        ))
    );

    println!();

    // Current active Zig version
    let active_zig = app.get_active_version();

    println!(
        "Current active Zig: {}{opt}",
        Paint::yellow(&active_zig.map_or_else(|| "none".to_string(), |v| v.to_string())),
        opt = if active_zig.is_none() {
            &format!(
                " (use {} to set one | or run {})",
                Paint::blue("zv use <version>"),
                Paint::blue("zv setup")
            )
        } else {
            ""
        }
    );

    println!();

    // Help section
    println!("{}", Paint::cyan("Usage: zv.exe [COMMAND]"));
    println!();
    println!("{}", Paint::yellow("Commands:").bold());
    println!(
        "\t{}\tInitialize a new Zig project from lean or standard zig template",
        Paint::blue("init")
    );
    println!(
        "\t{}\tSelect which Zig version to use - master | latest | stable | <semver>",
        Paint::blue("use")
    );
    println!("\t{}\tList installed Zig versions", Paint::blue("list"));
    println!(
        "\t{}\tClean up Zig installations. Non-zv managed installations will not be affected",
        Paint::blue("clean")
    );
    println!(
        "\t{}\tSetup shell environment for zv (required to make zig binaries available in $PATH)",
        Paint::blue("setup")
    );
    println!(
        "\t{}\tSynchronize index, mirrors list and metadata for zv",
        Paint::blue("sync")
    );
    println!(
        "\t{}\tPrint this message or the help of the given subcommand(s)",
        Paint::blue("help")
    );
}
