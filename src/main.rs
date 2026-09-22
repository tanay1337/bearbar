mod app;
mod bar;
mod config;
mod error;
mod icons;
mod modules;
mod runtime;
mod services;
mod style;

use std::path::PathBuf;

use config::Config;
use error::{Error, Result};
use tracing_subscriber::EnvFilter;

fn main() {
    if let Err(error) = run() {
        eprintln!("bearbar: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("bearbar=info")),
        )
        .without_time()
        .init();

    let arguments = Arguments::parse()?;
    let config = Config::load(arguments.config.as_deref())?;

    if arguments.check_config {
        println!("configuration is valid");
        return Ok(());
    }

    app::run(config, arguments.config, arguments.style)
}

#[derive(Debug, Default)]
struct Arguments {
    config: Option<PathBuf>,
    style: Option<PathBuf>,
    check_config: bool,
}

impl Arguments {
    fn parse() -> Result<Self> {
        let mut parsed = Self::default();
        let mut args = std::env::args_os().skip(1);

        while let Some(arg) = args.next() {
            match arg.to_string_lossy().as_ref() {
                "--config" => {
                    parsed.config = Some(
                        args.next()
                            .map(PathBuf::from)
                            .ok_or(Error::MissingArgument("--config"))?,
                    );
                }
                "--style" => {
                    parsed.style = Some(
                        args.next()
                            .map(PathBuf::from)
                            .ok_or(Error::MissingArgument("--style"))?,
                    );
                }
                "--check-config" => parsed.check_config = true,
                "--version" | "-V" => {
                    println!("bearbar {}", env!("CARGO_PKG_VERSION"));
                    std::process::exit(0);
                }
                "--help" | "-h" => {
                    println!(
                        "bearbar {}\n\nUSAGE:\n    bearbar [--config PATH] [--style PATH] [--check-config]",
                        env!("CARGO_PKG_VERSION")
                    );
                    std::process::exit(0);
                }
                value => return Err(Error::UnknownArgument(value.to_owned())),
            }
        }

        Ok(parsed)
    }
}
