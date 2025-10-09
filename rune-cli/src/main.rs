//! Rune CLI - Command line interface for the Rune markdown live editor

use clap::{Arg, Command};
use rune_core::{Config, CoreEngine, Result};
use std::path::PathBuf;
use tracing::{info, Level};

/// CLI arguments structure
#[derive(Debug, Clone)]
pub struct Args {
    pub file: PathBuf,
    pub hostname: String,
    pub port: u16,
    pub config_file: Option<PathBuf>,
    pub plugins_dir: Option<PathBuf>,
    pub dev_mode: bool,
}

impl Args {
    /// Parse command line arguments
    pub fn parse() -> Self {
        let matches = Command::new("rune")
            .version("0.1.0")
            .about("Modular markdown live editor with plugin support")
            .arg(
                Arg::new("file")
                    .help("Markdown file to serve")
                    .required(true)
                    .index(1)
                    .value_parser(clap::value_parser!(PathBuf))
            )
            .arg(
                Arg::new("hostname")
                    .short('h')
                    .long("hostname")
                    .help("Hostname to bind to")
                    .default_value("127.0.0.1")
                    .value_parser(clap::value_parser!(String))
            )
            .arg(
                Arg::new("port")
                    .short('p')
                    .long("port")
                    .help("Port to bind to")
                    .default_value("3000")
                    .value_parser(clap::value_parser!(u16))
            )
            .arg(
                Arg::new("config")
                    .short('c')
                    .long("config")
                    .help("Configuration file path")
                    .value_parser(clap::value_parser!(PathBuf))
            )
            .arg(
                Arg::new("plugins-dir")
                    .long("plugins-dir")
                    .help("Directory containing plugins")
                    .value_parser(clap::value_parser!(PathBuf))
            )
            .arg(
                Arg::new("dev-mode")
                    .long("dev-mode")
                    .help("Enable development mode with enhanced logging")
                    .action(clap::ArgAction::SetTrue)
            )
            .get_matches();

        Self {
            file: matches.get_one::<PathBuf>("file").unwrap().clone(),
            hostname: matches.get_one::<String>("hostname").unwrap().clone(),
            port: *matches.get_one::<u16>("port").unwrap(),
            config_file: matches.get_one::<PathBuf>("config").cloned(),
            plugins_dir: matches.get_one::<PathBuf>("plugins-dir").cloned(),
            dev_mode: matches.get_flag("dev-mode"),
        }
    }

    /// Validate the arguments
    pub fn validate(&self) -> Result<()> {
        // Check if file exists
        if !self.file.exists() {
            return Err(rune_core::RuneError::config(format!(
                "File does not exist: {}",
                self.file.display()
            )));
        }

        // Check if file is a markdown file
        if let Some(extension) = self.file.extension() {
            if extension != "md" && extension != "markdown" {
                return Err(rune_core::RuneError::config(
                    "File must have .md or .markdown extension".to_string()
                ));
            }
        } else {
            return Err(rune_core::RuneError::config(
                "File must have .md or .markdown extension".to_string()
            ));
        }

        // Validate port range
        if self.port == 0 {
            return Err(rune_core::RuneError::config(
                "Port must be greater than 0".to_string()
            ));
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.dev_mode { Level::DEBUG } else { Level::INFO };
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    info!("Starting Rune markdown live editor");

    // Validate arguments
    if let Err(e) = args.validate() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    // Load configuration
    let mut config = if let Some(config_file) = &args.config_file {
        info!("Loading configuration from: {}", config_file.display());
        Config::from_file(config_file)?
    } else {
        info!("Using default configuration");
        Config::new()
    };

    // Override config with CLI arguments
    config.server.hostname = args.hostname.clone();
    config.server.port = args.port;

    // Validate configuration
    config.validate()?;

    // Create and initialize core engine
    let mut engine = CoreEngine::new(config)?;
    engine.initialize().await?;

    info!(
        "Rune server starting on {}:{} for file: {}",
        args.hostname,
        args.port,
        args.file.display()
    );

    // Set up graceful shutdown
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install CTRL+C signal handler");
        info!("Shutdown signal received");
    };

    // In a complete implementation, this would start the actual server
    // For now, we just wait for shutdown
    tokio::select! {
        _ = shutdown_signal => {
            info!("Shutting down gracefully...");
        }
    }

    // Shutdown the engine
    engine.shutdown().await?;
    info!("Rune shutdown complete");

    Ok(())
}