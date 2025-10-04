use crate::{
    App, Shell, UserConfig, ZigVersion, suggest,
    tools::{self, error},
};
use clap::{Parser, Subcommand};
use color_eyre::eyre::eyre;
use std::str::FromStr;
use yansi::Paint;
mod clean;
mod init;
mod list;
mod setup;
pub mod sync; // Make sync public so other modules can use check_and_update_zv_binary
mod r#use;
mod zig;
mod zls;

pub use zig::zig_main;
pub use zls::zls_main;

/// Represents the target for a clean operation
#[derive(Debug, Clone)]
pub enum CleanTarget {
    All,
    Downloads,
    Versions(Vec<ZigVersion>),
}

/// Parse clean target string into CleanTarget enum
fn parse_clean_target(s: &str) -> Result<CleanTarget, String> {
    match s.to_lowercase().as_str() {
        "all" => Ok(CleanTarget::All),
        "downloads" => Ok(CleanTarget::Downloads),
        _ => {
            // Try parsing as comma-separated version list
            let versions: Result<Vec<ZigVersion>, _> = s
                .split(',')
                .map(|v| ZigVersion::from_str(v.trim()))
                .collect();

            match versions {
                Ok(vers) if !vers.is_empty() => Ok(CleanTarget::Versions(vers)),
                Ok(_) => Err("No valid versions provided".to_string()),
                Err(e) => Err(format!("Invalid version format: {}", e)),
            }
        }
    }
}

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
    })
    .await?;

    match zv_cli.command {
        Some(cmd) => cmd.execute(app, using_env).await?,
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
        /// Force using ziglang.org as a download source. Default is to use community mirrors.
        #[arg(
            long = "force-ziglang",
            short = 'f',
            long_help = "Force using ziglang.org as a download source. Default is to use community mirrors."
        )]
        force_ziglang: bool,
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
    Clean {
        /// Clean all versions except the specified ones (comma-separated)
        #[arg(
            long = "except",
            value_delimiter = ',',
            value_parser = clap::value_parser!(ZigVersion),
            help = "Clean all except specified versions (comma-separated)",
            long_help = "Clean all installed versions except the ones specified.\n\
                         Accepts comma-separated list of versions.\n\
                         Examples: --except 0.13.0,0.14.0 or --except master"
        )]
        except: Vec<ZigVersion>,

        /// Clean outdated master versions, keeping only the latest
        #[arg(
            long = "outdated",
            help = "Clean outdated master versions (keeps latest)",
            long_help = "Clean outdated master versions, keeping only the latest.\n\
                         If used with a target 'master', cleans master versions.\n\
                         If used alone, defaults to cleaning master versions."
        )]
        outdated: bool,

        /// Target to clean: 'all', 'downloads', version(s), or 'master'
        #[arg(
            value_parser = parse_clean_target,
            help = "What to clean: 'all', 'downloads', version(s), or omit for all",
            long_help = "Specify what to clean:\n\
                         • all          - Clean everything\n\
                         • downloads    - Clean downloads directory only\n\
                         • <version>    - Clean specific version (e.g., 0.13.0, master)\n\
                         • <v1,v2,...>  - Clean multiple versions (comma-separated)\n\
                         • master       - Clean all master versions (use with --outdated to keep latest)"
        )]
        target: Option<CleanTarget>,
    },

    /// Setup shell environment for zv (required to make zig binaries available in $PATH)
    ///
    /// Interactive mode is enabled by default, providing clear prompts about system changes.
    /// Interactive mode is automatically disabled in CI environments, when TERM=dumb,
    /// or when TTY is not available.
    Setup {
        /// Show what would be changed without making any modifications
        #[arg(
            long,
            alias = "dry",
            short = 'd',
            help = "Preview changes without applying them"
        )]
        dry_run: bool,
        /// Optional: Specify a specific zig version to set up. By default we don't install any version.
        #[arg(
            long,
            alias = "version",
            short = 'v',
            value_parser = clap::value_parser!(ZigVersion),
        )]
        default_version: Option<ZigVersion>,
        /// Disable interactive prompts and use default choices for automation
        #[arg(
            long = "no-interactive",
            help = "Disable interactive prompts and use default choices for automation",
            long_help = "Disable interactive prompts and use default choices for automation.\n\
                         Interactive mode is automatically disabled in CI environments,\n\
                         when TERM=dumb, or when TTY is not available."
        )]
        no_interactive: bool,
    },

    /// Synchronize index, mirrors list and metadata for zv.
    Sync,
}

impl Commands {
    pub(crate) async fn execute(self, mut app: App, using_env: bool) -> super::Result<()> {
        match self {
            Commands::Init { project_name, zig } => {
                use crate::{Template, TemplateType};
                if zig {
                    init::init_project(
                        Template::new(
                            project_name,
                            TemplateType::Zig(app.zv_zig().ok_or_else(|| {
                                tools::error("Cannot use `zig init` for template instantiation.");
                                suggest!(
                                    "You can install a compatible Zig version with {}",
                                    cmd = "zv use <version>"
                                );
                                suggest!(
                                    "Also make sure you've run {} to set up your shell environment",
                                    cmd = "zv setup"
                                );
                                eyre!("No Zig executable found")
                            })?),
                        ),
                        &app,
                    )
                } else {
                    init::init_project(Template::new(project_name, TemplateType::Embedded), &app)
                }
            }
            Commands::Use {
                version,
                force_ziglang,
            } => match version {
                Some(version) => r#use::use_version(version, &mut app, force_ziglang).await,
                None => {
                    error("Version must be specified. e.g., `zv use latest` or `zv use 0.15.1`");
                    std::process::exit(2);
                }
            },
            Commands::List => list::list_versions(&mut app).await,
            Commands::Clean {
                except,
                outdated,
                target,
            } => clean::clean(&mut app, target, except, outdated).await,
            Commands::Setup {
                dry_run,
                default_version,
                no_interactive,
            } => {
                setup::setup_shell(
                    &mut app,
                    using_env,
                    dry_run,
                    no_interactive,
                    default_version,
                )
                .await
            }
            Commands::Sync => sync::sync(&mut app).await,
        }
    }
}

fn get_zv_lines() -> Vec<&'static str> {
    vec![
        "███████╗██╗   ██╗ ",
        "╚══███╔╝██║   ██║ ",
        "  ███╔╝ ██║   ██║ ",
        " ███╔╝  ██║   ██║ ",
        "███████╗╚████╔╝█  ",
        "╚══════╝  ╚══╝    ",
    ]
}

fn zv_line_with_color(line: &str, color: yansi::Color) -> String {
    Paint::new(line).fg(color).to_string()
}

fn print_welcome_message(app: App) {
    use target_lexicon::HOST;
    let (color1, color2) = get_random_color_scheme();

    // Parse the target triplet (format: arch-platform-os)
    let architecture = HOST.architecture;
    let source_set = app.source_set;
    let os = HOST.operating_system;
    let zv_version = env!("CARGO_PKG_VERSION");

    // Only show ASCII art if we're attached to a TTY
    if tools::is_tty() {
        let zv_lines = get_zv_lines();
        let info_lines = vec![
            format!("Architecture: {architecture}"),
            format!("OS: {os}"),
            format!(
                "ZV status: {}",
                if source_set {
                    Paint::green("✔ Ready to Use").to_string()
                } else {
                    format!(
                        "{} {}",
                        Paint::red("Not in PATH."),
                        "Run ".to_string()
                            + &Paint::blue("zv setup").to_string()
                            + " to set ZV in PATH & install a default Zig version"
                    )
                }
            ),
            format!("ZV directory: {}", app.path().display()),
            format!("ZV Version: {zv_version}"),
            format!(
                "Shell: {}",
                app.shell.as_ref().map_or(Shell::detect(), |s| s.clone())
            ),
        ];

        // Add profile line if available
        let mut all_info_lines = info_lines;
        if let Some(profile) = std::env::var("PROFILE").ok().filter(|p| !p.is_empty()) {
            all_info_lines.push(format!("Profile: {profile}"));
        }

        println!();
        for (i, zv_line) in zv_lines.iter().enumerate() {
            let colored_line = if i < zv_lines.len() / 2 {
                zv_line_with_color(zv_line, color1)
            } else {
                zv_line_with_color(zv_line, color2)
            };

            let info_part = if i < all_info_lines.len() {
                format!("    {}", all_info_lines[i])
            } else {
                String::new()
            };

            println!("{}{}", colored_line, info_part);
        }

        // Print any remaining info lines if there are more info lines than ASCII art lines
        for remaining_info in all_info_lines.iter().skip(zv_lines.len()) {
            println!("                     {}", remaining_info);
        }

        println!();
    } else {
        // When not in TTY, show minimal info
        println!("zv - Zig Version Manager");
        println!("Architecture: {architecture}");
        println!("OS: {os}");
        println!(
            "ZV Setup: {}",
            if source_set {
                "Ready to Use"
            } else {
                "Not in PATH"
            }
        );
        println!("ZV directory: {}", app.path().display());
        println!(
            "Shell: {}",
            app.shell.as_ref().map_or(Shell::detect(), |s| s.clone())
        );
        if let Some(profile) = std::env::var("PROFILE").ok().filter(|p| !p.is_empty()) {
            println!("Profile: {profile}");
        }
        println!("ZV Version: {}", zv_version);
        println!();
    }

    // Current active Zig version
    let active_zig: Option<ZigVersion> = app.get_active_version();

    let active_zig_str = active_zig
        .as_ref()
        .map_or_else(|| "none".to_string(), |v| v.to_string());
    let help_text = if active_zig.is_none() {
        format!(
            " (use {} to set one | or run {} to get started)",
            Paint::blue("zv use <version>"),
            Paint::blue("zv setup")
        )
    } else {
        String::new()
    };

    println!(
        "Current active Zig: {}{}",
        Paint::yellow(&active_zig_str),
        help_text
    );
    println!();

    // Help section
    if cfg!(windows) {
        println!("{}", Paint::cyan("Usage: zv.exe [COMMAND]"));
    } else {
        println!("{}", Paint::cyan("Usage: zv [COMMAND]"));
    }
    println!();
    println!("{}", Paint::yellow("Commands:").bold());

    let print_command = |cmd: &str, desc: &str| {
        println!("\t{:<12}\t{}", Paint::green(cmd), desc);
    };

    print_command(
        "init",
        "Initialize a new Zig project from lean or standard zig template",
    );
    print_command(
        "use",
        "Select which Zig version to use - master | latest | stable | <semver>",
    );
    print_command("list  | ls", "List installed Zig versions");
    print_command(
        "clean | rm",
        "Clean up Zig installations. Non-zv managed installations will not be affected",
    );
    print_command(
        "setup",
        "Setup shell environment for zv with interactive prompts (use --no-interactive to disable)",
    );
    print_command(
        "sync",
        "Synchronize index, mirrors list and metadata for zv",
    );
    print_command(
        "help",
        "Print this message or the help of the given subcommand(s)",
    );
}

// Define some stylish two-tone color pairs
fn get_random_color_scheme() -> (yansi::Color, yansi::Color) {
    use rand::Rng;
    let schemes = [
        (
            yansi::Color::Rgb(255, 100, 0), // Bright Orange
            yansi::Color::Rgb(0, 191, 255), // Deep Sky Blue
        ), // Orange → Blue
        (
            yansi::Color::Rgb(255, 215, 0), // Gold
            yansi::Color::Rgb(75, 0, 130),  // Indigo
        ), // Gold → Indigo
        (
            yansi::Color::Rgb(220, 20, 60), // Crimson
            yansi::Color::Rgb(0, 255, 255), // Cyan
        ), // Crimson → Cyan
        (
            yansi::Color::Rgb(247, 147, 26),
            yansi::Color::Rgb(255, 255, 255),
        ), // Zig Orange → White
    ];

    let mut rng = rand::rng();
    schemes[rng.random_range(0..schemes.len())]
}
