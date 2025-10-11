//! Rune CLI - Command line interface for the Rune markdown live editor

use clap::{Arg, Command};
use rune_core::{Config, CoreEngine, Result, RuneError};
use std::path::PathBuf;
use tracing::{error, info, warn, Level};

/// CLI arguments structure
#[derive(Debug, Clone)]
pub struct Args {
    pub file: PathBuf,
    pub hostname: String,
    pub port: u16,
    pub config_file: Option<PathBuf>,
    pub plugins_dir: Option<PathBuf>,
    pub dev_mode: bool,
    pub list_plugins: bool,
    pub validate_config: bool,
}

impl Args {
    /// Parse command line arguments
    pub fn parse() -> Self {
        let matches = Command::new("rune")
            .version("0.1.0")
            .author("Rune Team")
            .about("Modular markdown live editor with plugin support")
            .long_about(
                "Rune is a modular markdown live editor that provides real-time preview \
                capabilities with a plugin-based architecture. It serves markdown files \
                through a web interface with live reload functionality."
            )
            .arg(
                Arg::new("file")
                    .help("Markdown file to serve (.md or .markdown)")
                    .long_help(
                        "Path to the markdown file to serve. The file must exist and have \
                        a .md or .markdown extension. The web interface will display the \
                        rendered content and automatically update when the file changes. \
                        Not required for utility commands like --list-plugins or --validate-config."
                    )
                    .required_unless_present_any(["list-plugins", "validate-config"])
                    .index(1)
                    .value_parser(clap::value_parser!(PathBuf)),
            )
            .arg(
                Arg::new("hostname")
                    .short('H')
                    .long("hostname")
                    .help("Hostname or IP address to bind the server to")
                    .long_help(
                        "The hostname or IP address where the server will listen for connections. \
                        Use '0.0.0.0' to bind to all available interfaces, or '127.0.0.1' for \
                        localhost only (default)."
                    )
                    .default_value("127.0.0.1")
                    .value_parser(clap::value_parser!(String)),
            )
            .arg(
                Arg::new("port")
                    .short('p')
                    .long("port")
                    .help("Port number to bind the server to (1-65535)")
                    .long_help(
                        "The port number where the server will listen for HTTP connections. \
                        Must be between 1 and 65535. If the port is already in use, the \
                        application will display an error and suggest alternatives."
                    )
                    .default_value("3000")
                    .value_parser(clap::value_parser!(u16)),
            )
            .arg(
                Arg::new("config")
                    .short('c')
                    .long("config")
                    .help("Path to configuration file (JSON format)")
                    .long_help(
                        "Path to a JSON configuration file that contains plugin settings, \
                        server options, and other system configuration. CLI arguments will \
                        override settings from the configuration file."
                    )
                    .value_parser(clap::value_parser!(PathBuf)),
            )
            .arg(
                Arg::new("plugins-dir")
                    .long("plugins-dir")
                    .help("Directory containing plugin files")
                    .long_help(
                        "Path to a directory containing plugin files. Plugins in this directory \
                        will be automatically discovered and loaded based on the configuration. \
                        If not specified, the system will look for plugins in default locations."
                    )
                    .value_parser(clap::value_parser!(PathBuf)),
            )
            .arg(
                Arg::new("dev-mode")
                    .long("dev-mode")
                    .help("Enable development mode with enhanced logging and debugging")
                    .long_help(
                        "Enable development mode which provides enhanced logging, plugin \
                        hot-reloading, and additional debugging information. This mode is \
                        useful for plugin development and troubleshooting."
                    )
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                Arg::new("list-plugins")
                    .long("list-plugins")
                    .help("List available plugins and exit")
                    .long_help(
                        "Display information about available plugins including their status, \
                        version, and dependencies, then exit without starting the server."
                    )
                    .action(clap::ArgAction::SetTrue),
            )
            .arg(
                Arg::new("validate-config")
                    .long("validate-config")
                    .help("Validate configuration file and exit")
                    .long_help(
                        "Validate the configuration file syntax and plugin dependencies, \
                        then exit without starting the server. Useful for testing \
                        configuration changes."
                    )
                    .action(clap::ArgAction::SetTrue),
            )
            .after_help(
                "EXAMPLES:\n    \
                rune README.md                           Start server with default settings\n    \
                rune -p 8080 -h 0.0.0.0 docs/guide.md   Bind to all interfaces on port 8080\n    \
                rune --config config.json README.md     Use custom configuration file\n    \
                rune --dev-mode --plugins-dir ./plugins README.md  Development mode with custom plugins\n    \
                rune --list-plugins                      Show available plugins\n    \
                rune --validate-config --config config.json  Validate configuration\n\n\
                For more information, visit: https://github.com/rune-rs/rune"
            )
            .get_matches();

        Self {
            file: matches
                .get_one::<PathBuf>("file")
                .cloned()
                .unwrap_or_default(),
            hostname: matches.get_one::<String>("hostname").unwrap().clone(),
            port: *matches.get_one::<u16>("port").unwrap(),
            config_file: matches.get_one::<PathBuf>("config").cloned(),
            plugins_dir: matches.get_one::<PathBuf>("plugins-dir").cloned(),
            dev_mode: matches.get_flag("dev-mode"),
            list_plugins: matches.get_flag("list-plugins"),
            validate_config: matches.get_flag("validate-config"),
        }
    }

    /// Validate the arguments with detailed error messages
    pub fn validate(&self) -> Result<()> {
        // Skip file validation for utility commands
        if self.list_plugins || self.validate_config {
            return self.validate_utility_args();
        }

        // Check if file exists
        if !self.file.exists() {
            return Err(RuneError::config(format!(
                "Markdown file not found: {}\n\n\
                Please check that:\n\
                • The file path is correct\n\
                • You have read permissions for the file\n\
                • The file hasn't been moved or deleted\n\n\
                Example: rune README.md",
                self.file.display()
            )));
        }

        // Check if it's actually a file (not a directory)
        if self.file.is_dir() {
            return Err(RuneError::config(format!(
                "Path is a directory, not a file: {}\n\n\
                Please specify a markdown file, not a directory.\n\n\
                Example: rune {}/README.md",
                self.file.display(),
                self.file.display()
            )));
        }

        // Check if file is readable
        match std::fs::File::open(&self.file) {
            Ok(_) => {}
            Err(e) => {
                return Err(RuneError::config(format!(
                    "Cannot read file: {}\n\
                    Error: {}\n\n\
                    Please check that you have read permissions for this file.",
                    self.file.display(),
                    e
                )));
            }
        }

        // Check if file is a markdown file
        if let Some(extension) = self.file.extension() {
            let ext_str = extension.to_string_lossy().to_lowercase();
            if ext_str != "md" && ext_str != "markdown" {
                return Err(RuneError::config(format!(
                    "File must be a markdown file (.md or .markdown): {}\n\n\
                    Current extension: .{}\n\n\
                    Supported extensions:\n\
                    • .md\n\
                    • .markdown\n\n\
                    Example: rune document.md",
                    self.file.display(),
                    ext_str
                )));
            }
        } else {
            return Err(RuneError::config(format!(
                "File must have a markdown extension (.md or .markdown): {}\n\n\
                Please rename your file to include a proper extension.\n\n\
                Example: mv {} {}.md",
                self.file.display(),
                self.file.display(),
                self.file.display()
            )));
        }

        // Validate port range
        if self.port == 0 {
            return Err(RuneError::config(
                "Port number cannot be 0.\n\n\
                Please specify a valid port number between 1 and 65535.\n\n\
                Examples:\n\
                • rune -p 3000 README.md  (default port)\n\
                • rune -p 8080 README.md  (common alternative)\n\
                • rune -p 8000 README.md  (development port)"
                    .to_string(),
            ));
        }

        // Validate hostname format (basic check)
        if self.hostname.is_empty() {
            return Err(RuneError::config(
                "Hostname cannot be empty.\n\n\
                Valid hostname examples:\n\
                • 127.0.0.1 (localhost only)\n\
                • 0.0.0.0 (all interfaces)\n\
                • localhost (local machine)"
                    .to_string(),
            ));
        }

        // Validate config file if provided
        if let Some(config_file) = &self.config_file {
            if !config_file.exists() {
                return Err(RuneError::config(format!(
                    "Configuration file not found: {}\n\n\
                    Please check that:\n\
                    • The config file path is correct\n\
                    • You have read permissions for the file\n\
                    • The file exists and is accessible\n\n\
                    Example: rune --config config.json README.md",
                    config_file.display()
                )));
            }

            if config_file.is_dir() {
                return Err(RuneError::config(format!(
                    "Configuration path is a directory, not a file: {}\n\n\
                    Please specify a JSON configuration file.\n\n\
                    Example: rune --config {}/config.json README.md",
                    config_file.display(),
                    config_file.display()
                )));
            }
        }

        // Validate plugins directory if provided
        if let Some(plugins_dir) = &self.plugins_dir {
            if !plugins_dir.exists() {
                warn!(
                    "Plugins directory does not exist: {}\n\
                    The directory will be created if needed.",
                    plugins_dir.display()
                );
            } else if !plugins_dir.is_dir() {
                return Err(RuneError::config(format!(
                    "Plugins path is not a directory: {}\n\n\
                    Please specify a directory containing plugin files.\n\n\
                    Example: rune --plugins-dir ./plugins README.md",
                    plugins_dir.display()
                )));
            }
        }

        Ok(())
    }

    /// Validate arguments for utility commands (list-plugins, validate-config)
    fn validate_utility_args(&self) -> Result<()> {
        // For utility commands, we only need to validate the config file if provided
        if let Some(config_file) = &self.config_file {
            if !config_file.exists() {
                return Err(RuneError::config(format!(
                    "Configuration file not found: {}\n\n\
                    Cannot validate a configuration file that doesn't exist.\n\n\
                    Example: rune --validate-config --config config.json",
                    config_file.display()
                )));
            }
        }

        Ok(())
    }

    /// Load configuration with proper error handling and CLI overrides
    pub fn load_config(&self) -> Result<Config> {
        let mut config = if let Some(config_file) = &self.config_file {
            info!("Loading configuration from: {}", config_file.display());

            match Config::from_file(config_file) {
                Ok(config) => {
                    info!("Configuration loaded successfully");
                    config
                }
                Err(e) => {
                    return Err(RuneError::config(format!(
                        "Failed to load configuration file: {}\n\
                        Error: {}\n\n\
                        Please check that:\n\
                        • The file contains valid JSON\n\
                        • All required fields are present\n\
                        • Plugin configurations are correct\n\n\
                        You can validate your config with: rune --validate-config --config {}",
                        config_file.display(),
                        e,
                        config_file.display()
                    )));
                }
            }
        } else {
            info!("Using default configuration");
            Config::new()
        };

        // Override config with CLI arguments
        config.server.hostname = self.hostname.clone();
        config.server.port = self.port;

        // Add plugins directory to global settings if provided
        if let Some(plugins_dir) = &self.plugins_dir {
            config.set_global_setting(
                "plugins_directory".to_string(),
                plugins_dir.to_string_lossy().to_string(),
            )?;
        }

        // Set development mode
        config.set_global_setting("dev_mode".to_string(), self.dev_mode)?;

        Ok(config)
    }

    /// Check if port is available
    pub fn check_port_availability(&self) -> Result<()> {
        use std::net::{TcpListener, ToSocketAddrs};

        let addr = format!("{}:{}", self.hostname, self.port);

        // Try to resolve the address first
        let socket_addrs: Vec<_> = match addr.to_socket_addrs() {
            Ok(addrs) => addrs.collect(),
            Err(e) => {
                return Err(RuneError::config(format!(
                    "Invalid hostname '{}': {}\n\n\
                    Please use a valid hostname or IP address:\n\
                    • 127.0.0.1 (localhost)\n\
                    • 0.0.0.0 (all interfaces)\n\
                    • localhost (local machine)",
                    self.hostname, e
                )));
            }
        };

        if socket_addrs.is_empty() {
            return Err(RuneError::config(format!(
                "Could not resolve hostname: {}\n\n\
                Please check your network configuration and try again.",
                self.hostname
            )));
        }

        // Try to bind to the port
        match TcpListener::bind(&addr) {
            Ok(_) => {
                info!("Port {} is available on {}", self.port, self.hostname);
                Ok(())
            }
            Err(e) => {
                let suggested_ports = [3001, 3002, 8000, 8080, 8888];
                let mut available_ports = Vec::new();

                // Check for alternative ports
                for &port in &suggested_ports {
                    if port != self.port {
                        let test_addr = format!("{}:{}", self.hostname, port);
                        if TcpListener::bind(&test_addr).is_ok() {
                            available_ports.push(port);
                        }
                    }
                }

                let suggestions = if available_ports.is_empty() {
                    "Try using a different port number.".to_string()
                } else {
                    format!(
                        "Try one of these available ports:\n{}",
                        available_ports
                            .iter()
                            .map(|p| format!(
                                "  rune -p {} {} {}",
                                p,
                                self.hostname,
                                self.file.display()
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };

                Err(RuneError::config(format!(
                    "Port {} is already in use on {}\n\
                    Error: {}\n\n\
                    {}\n\n\
                    You can also:\n\
                    • Stop the process using port {}\n\
                    • Use 'lsof -i :{} ' to find what's using the port\n\
                    • Choose a different port with -p <port>",
                    self.port, self.hostname, e, suggestions, self.port, self.port
                )))
            }
        }
    }
}

/// List available plugins and their information
async fn list_plugins(args: &Args) -> Result<()> {
    println!("🔌 Available Plugins\n");

    let config = args.load_config()?;
    let engine = CoreEngine::new(config)?;

    // Get plugin information from the registry
    let plugin_registry = engine.plugin_registry();
    let plugins = plugin_registry.list_plugins();

    if plugins.is_empty() {
        println!("No plugins found.");

        if let Some(plugins_dir) = &args.plugins_dir {
            println!("\nSearched in: {}", plugins_dir.display());
        } else {
            println!("\nTo add plugins, use: --plugins-dir <directory>");
        }

        return Ok(());
    }

    for plugin in plugins {
        println!("📦 {}", plugin.name);
        println!("   Version: {}", plugin.version);
        println!("   Status: {:?}", plugin.status);

        if !plugin.dependencies.is_empty() {
            println!("   Dependencies: {}", plugin.dependencies.join(", "));
        }

        println!(
            "   Loaded: {}",
            plugin.load_time.elapsed().unwrap_or_default().as_secs()
        );
        println!();
    }

    Ok(())
}

/// Validate configuration file
async fn validate_config(args: &Args) -> Result<()> {
    println!("🔍 Validating Configuration\n");

    let config_file = args.config_file.as_ref().ok_or_else(|| {
        RuneError::config(
            "No configuration file specified.\n\n\
            Use: rune --validate-config --config <file.json>"
                .to_string(),
        )
    })?;

    println!("Configuration file: {}", config_file.display());

    // Load and validate the configuration
    match args.load_config() {
        Ok(config) => {
            println!("✅ Configuration is valid\n");

            // Display configuration summary
            println!("Server Configuration:");
            println!("  Hostname: {}", config.server.hostname);
            println!("  Port: {}", config.server.port);
            println!("  CORS Enabled: {}", config.server.cors_enabled);
            println!("  WebSocket Enabled: {}", config.server.websocket_enabled);

            if !config.plugins.is_empty() {
                println!("\nPlugin Configuration:");
                for plugin in &config.plugins {
                    let status = if plugin.enabled { "✅" } else { "❌" };
                    println!(
                        "  {} {} ({})",
                        status,
                        plugin.name,
                        plugin.version.as_deref().unwrap_or("latest")
                    );

                    if !plugin.dependencies.is_empty() {
                        println!("    Dependencies: {}", plugin.dependencies.join(", "));
                    }
                }
            }

            if !config.global_settings.is_empty() {
                println!("\nGlobal Settings:");
                for (key, value) in &config.global_settings {
                    println!("  {}: {}", key, value);
                }
            }

            println!("\n✅ All validations passed!");
        }
        Err(e) => {
            println!("❌ Configuration validation failed\n");
            return Err(e);
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize logging early for utility commands
    let log_level = if args.dev_mode {
        Level::DEBUG
    } else {
        Level::INFO
    };
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    // Handle utility commands first
    if args.list_plugins {
        return match list_plugins(&args).await {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("❌ Failed to list plugins: {}", e);
                std::process::exit(1);
            }
        };
    }

    if args.validate_config {
        return match validate_config(&args).await {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("❌ Configuration validation failed:\n{}", e);
                std::process::exit(1);
            }
        };
    }

    // For server mode, validate all arguments
    if let Err(e) = args.validate() {
        eprintln!("❌ Invalid arguments:\n{}", e);
        std::process::exit(1);
    }

    // Check port availability early
    if let Err(e) = args.check_port_availability() {
        eprintln!("❌ Port check failed:\n{}", e);
        std::process::exit(1);
    }

    info!("🚀 Starting Rune markdown live editor");

    // Load configuration with enhanced error handling
    let config = match args.load_config() {
        Ok(config) => config,
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Validate configuration
    if let Err(e) = config.validate() {
        error!("Configuration validation failed: {}", e);
        std::process::exit(1);
    }

    // Create and initialize core engine
    let mut engine = match CoreEngine::new(config) {
        Ok(engine) => engine,
        Err(e) => {
            error!("Failed to create core engine: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = engine.initialize().await {
        error!("Failed to initialize core engine: {}", e);
        std::process::exit(1);
    }

    // Display startup information
    println!("🌟 Rune Markdown Live Editor");
    println!("📁 File: {}", args.file.display());
    println!("🌐 Server: http://{}:{}", args.hostname, args.port);

    if args.dev_mode {
        println!("🔧 Development mode enabled");
    }

    if args.plugins_dir.is_some() {
        println!(
            "🔌 Custom plugins directory: {}",
            args.plugins_dir.as_ref().unwrap().display()
        );
    }

    println!("📡 WebSocket live reload enabled");
    println!("\n✨ Server ready! Press Ctrl+C to stop.\n");

    info!(
        "Rune server started successfully on {}:{} for file: {}",
        args.hostname,
        args.port,
        args.file.display()
    );

    // Set up graceful shutdown
    let shutdown_signal = async {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Shutdown signal received");
                println!("\n🛑 Shutting down gracefully...");
            }
            Err(e) => {
                error!("Failed to listen for shutdown signal: {}", e);
            }
        }
    };

    // In a complete implementation, this would start the actual server
    // For now, we just wait for shutdown
    tokio::select! {
        _ = shutdown_signal => {
            info!("Initiating graceful shutdown...");
        }
    }

    // Shutdown the engine
    if let Err(e) = engine.shutdown().await {
        error!("Error during shutdown: {}", e);
        std::process::exit(1);
    }

    println!("✅ Rune shutdown complete");
    info!("Rune shutdown complete");

    Ok(())
}
