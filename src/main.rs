use app::App;
use cli::ZvCli;
use color_eyre::{
    Result,
    config::{HookBuilder, Theme},
    eyre::Context as _,
};

#[tokio::main]
async fn main() -> Result<()> {
    yansi::whenever(yansi::Condition::TTY_AND_COLOR);
    if yansi::is_enabled() {
        color_eyre::install()?;
    } else {
        HookBuilder::default().theme(Theme::new()).install()?;
    }
    let zv_cli = <ZvCli as clap::Parser>::parse();

    #[cfg(feature = "dotenv")]
    dotenv::dotenv().ok();

    #[cfg(feature = "log")]
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "zv=info".into()))
        .with_writer(std::io::stderr)
        .init();

    let (zv_dir, using_env) = tools::fetch_zv_dir()?;

    // TODO: Allow force flags to skip prompts in ZvCli
    // let allow_shell = zv_cli.allow_shell || zv_cli.force;
    // let force = zv_cli.force;
    // let g = Genie { allow_shell, force };

    // Init ZV_DIR
    match zv_dir.try_exists() {
        Ok(true) => {
            if !zv_dir.is_dir() {
                tools::error(format!(
                    "zv directory exists but is not a directory: {}. Please check ZV_DIR env var. Aborting...",
                    zv_dir.display()
                ));
                std::process::exit(1);
            }
        }
        Ok(false) => {
            if using_env {
                std::fs::create_dir_all(&zv_dir)
                    .map_err(ZvError::Io)
                    .wrap_err_with(|| {
                        format!(
                            "Error creating ZV_DIR from env var ZV_DIR={}",
                            std::env::var("ZV_DIR").expect("Handled in zv_fetch_dir()")
                        )
                    })?;
            } else {
                // Using fallback path $HOME/.zv (or $CWD/.zv in rare fallback)
                std::fs::create_dir(&zv_dir)
                    .map_err(ZvError::Io)
                    .wrap_err_with(|| {
                        format!("Failed to create default .zv at {}", zv_dir.display())
                    })?;
            }
        }
        Err(e) => {
            tools::error(format!(
                "Failed to check zv directory at {:?}",
                zv_dir.display(),
            ));
            return Err(ZvError::Io(e).into());
        }
    };
    let zv_dir = std::fs::canonicalize(&zv_dir).map_err(ZvError::Io)?;

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

mod app;
mod cli;
mod shell;
mod templates;
mod tools;
mod types;

pub use shell::*;
pub use templates::*;
pub use types::*;
