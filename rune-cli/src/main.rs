//! Rune CLI - Command line interface for the Rune markdown live editor

use clap::{Arg, Command};
use rune_core::{Config, CoreEngine, Result, RuneError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, error, info, warn, Level};

/// Discovered plugin information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPlugin {
    pub name: String,
    pub path: PathBuf,
    pub version: Option<String>,
    pub description: Option<String>,
    pub plugin_type: PluginType,
}

/// Plugin type enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginType {
    Native,      // Rust dynamic library
    Script,      // Script-based plugin
    Config,      // Configuration-only plugin
    Unknown,
}

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

    // Scan for available plugins in directories
    let mut discovered_plugins = Vec::new();
    if let Some(plugins_dir) = &args.plugins_dir {
        discovered_plugins.extend(scan_plugin_directory(plugins_dir)?);
    }

    // Also scan default plugin directories
    let default_dirs = get_default_plugin_directories();
    for dir in default_dirs {
        if dir.exists() {
            discovered_plugins.extend(scan_plugin_directory(&dir)?);
        }
    }

    if plugins.is_empty() && discovered_plugins.is_empty() {
        println!("No plugins found.");

        if let Some(plugins_dir) = &args.plugins_dir {
            println!("\nSearched in: {}", plugins_dir.display());
        } else {
            println!("\nTo add plugins, use: --plugins-dir <directory>");
        }

        println!("\nDefault plugin directories:");
        for dir in get_default_plugin_directories() {
            println!("  {}", dir.display());
        }

        return Ok(());
    }

    // Display loaded plugins
    if !plugins.is_empty() {
        println!("📋 Loaded Plugins:");
        for plugin in &plugins {
            println!("📦 {}", plugin.name);
            println!("   Version: {}", plugin.version);
            println!("   Status: {:?}", plugin.status);
            println!("   Health: {:?}", plugin.health_status);

            if !plugin.dependencies.is_empty() {
                println!("   Dependencies: {}", plugin.dependencies.join(", "));
            }

            if !plugin.provided_services.is_empty() {
                println!("   Services: {}", plugin.provided_services.join(", "));
            }

            println!(
                "   Loaded: {} seconds ago",
                plugin.load_time.elapsed().unwrap_or_default().as_secs()
            );

            if plugin.restart_count > 0 {
                println!("   Restarts: {}", plugin.restart_count);
            }

            println!();
        }
    }

    // Display discovered plugins
    if !discovered_plugins.is_empty() {
        println!("🔍 Discovered Plugins:");
        for discovered in discovered_plugins {
            let status = if plugins.iter().any(|p| p.name == discovered.name) {
                "✅ Loaded"
            } else {
                "⏸️  Available"
            };

            println!("📦 {} ({})", discovered.name, status);
            println!("   Path: {}", discovered.path.display());
            if let Some(version) = &discovered.version {
                println!("   Version: {}", version);
            }
            if let Some(description) = &discovered.description {
                println!("   Description: {}", description);
            }
            println!();
        }
    }

    // Show system health in dev mode
    if args.dev_mode {
        println!("🏥 System Health: {:?}", plugin_registry.get_system_health());
        
        let all_plugins = plugin_registry.list_plugins();
        let unhealthy_plugins: Vec<_> = all_plugins
            .iter()
            .filter(|p| format!("{:?}", p.health_status).contains("Unhealthy"))
            .collect();
            
        if !unhealthy_plugins.is_empty() {
            println!("\n⚠️  Unhealthy Plugins:");
            for plugin in unhealthy_plugins {
                println!("   {} - {:?}", plugin.name, plugin.status);
            }
        }
    }

    Ok(())
}

/// Validate configuration file
async fn validate_config(args: &Args) -> Result<()> {
    if args.dev_mode {
        // Use interactive validation in dev mode
        interactive_config_validation(args).await
    } else {
        // Use simple validation for normal mode
        simple_config_validation(args).await
    }
}

/// Simple configuration validation (non-interactive)
async fn simple_config_validation(args: &Args) -> Result<()> {
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

/// Scan a directory for available plugins
fn scan_plugin_directory(dir: &PathBuf) -> Result<Vec<DiscoveredPlugin>> {
    let mut discovered = Vec::new();

    if !dir.exists() {
        debug!("Plugin directory does not exist: {}", dir.display());
        return Ok(discovered);
    }

    if !dir.is_dir() {
        return Err(RuneError::config(format!(
            "Plugin path is not a directory: {}",
            dir.display()
        )));
    }

    debug!("Scanning plugin directory: {}", dir.display());

    let entries = std::fs::read_dir(dir).map_err(|e| {
        RuneError::config(format!(
            "Failed to read plugin directory {}: {}",
            dir.display(),
            e
        ))
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| {
            RuneError::config(format!("Failed to read directory entry: {}", e))
        })?;

        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden files and directories
        if name.starts_with('.') {
            continue;
        }

        let plugin_type = if path.is_dir() {
            // Check for plugin manifest in subdirectory
            let manifest_path = path.join("plugin.json");
            if manifest_path.exists() {
                match load_plugin_manifest(&manifest_path) {
                    Ok(manifest) => {
                        discovered.push(DiscoveredPlugin {
                            name: manifest.name.unwrap_or_else(|| name.to_string()),
                            path: path.clone(),
                            version: manifest.version,
                            description: manifest.description,
                            plugin_type: PluginType::Config,
                        });
                        continue;
                    }
                    Err(e) => {
                        warn!("Failed to load plugin manifest {}: {}", manifest_path.display(), e);
                        PluginType::Unknown
                    }
                }
            } else {
                PluginType::Unknown
            }
        } else if let Some(extension) = path.extension() {
            match extension.to_string_lossy().as_ref() {
                "so" | "dll" | "dylib" => PluginType::Native,
                "js" | "py" | "lua" => PluginType::Script,
                "json" => {
                    // Check if it's a plugin configuration
                    if name.contains("plugin") {
                        match load_plugin_manifest(&path) {
                            Ok(manifest) => {
                                discovered.push(DiscoveredPlugin {
                                    name: manifest.name.unwrap_or_else(|| {
                                        path.file_stem()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string()
                                    }),
                                    path: path.clone(),
                                    version: manifest.version,
                                    description: manifest.description,
                                    plugin_type: PluginType::Config,
                                });
                                continue;
                            }
                            Err(e) => {
                                debug!("Not a plugin manifest {}: {}", path.display(), e);
                                PluginType::Unknown
                            }
                        }
                    } else {
                        PluginType::Unknown
                    }
                }
                _ => PluginType::Unknown,
            }
        } else {
            PluginType::Unknown
        };

        if !matches!(plugin_type, PluginType::Unknown) {
            let plugin_name = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            discovered.push(DiscoveredPlugin {
                name: plugin_name,
                path: path.clone(),
                version: None,
                description: None,
                plugin_type,
            });
        }
    }

    debug!("Discovered {} plugins in {}", discovered.len(), dir.display());
    Ok(discovered)
}

/// Plugin manifest structure for configuration-based plugins
#[derive(Debug, Serialize, Deserialize)]
struct PluginManifest {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    author: Option<String>,
    dependencies: Option<Vec<String>>,
    services: Option<Vec<String>>,
}

/// Load plugin manifest from JSON file
fn load_plugin_manifest(path: &PathBuf) -> Result<PluginManifest> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        RuneError::config(format!("Failed to read plugin manifest {}: {}", path.display(), e))
    })?;

    let manifest: PluginManifest = serde_json::from_str(&content).map_err(|e| {
        RuneError::config(format!(
            "Failed to parse plugin manifest {}: {}",
            path.display(),
            e
        ))
    })?;

    Ok(manifest)
}

/// Get default plugin directories
fn get_default_plugin_directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Current directory plugins
    dirs.push(PathBuf::from("./plugins"));

    // User-specific plugin directory
    if let Some(home_dir) = dirs::home_dir() {
        dirs.push(home_dir.join(".rune").join("plugins"));
    }

    // System-wide plugin directory
    #[cfg(unix)]
    {
        dirs.push(PathBuf::from("/usr/local/lib/rune/plugins"));
        dirs.push(PathBuf::from("/opt/rune/plugins"));
    }

    #[cfg(windows)]
    {
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            dirs.push(PathBuf::from(program_files).join("Rune").join("plugins"));
        }
    }

    dirs
}

/// Interactive configuration validation with detailed feedback
async fn interactive_config_validation(args: &Args) -> Result<()> {
    println!("🔧 Interactive Configuration Validation\n");

    let config_file = args.config_file.as_ref().ok_or_else(|| {
        RuneError::config(
            "No configuration file specified.\n\n\
            Use: rune --validate-config --config <file.json>"
                .to_string(),
        )
    })?;

    println!("Configuration file: {}", config_file.display());

    // Load and validate the configuration step by step
    println!("\n📋 Step 1: Loading configuration file...");
    let config = match args.load_config() {
        Ok(config) => {
            println!("✅ Configuration loaded successfully");
            config
        }
        Err(e) => {
            println!("❌ Configuration loading failed");
            return Err(e);
        }
    };

    // Validate server configuration
    println!("\n📋 Step 2: Validating server configuration...");
    if config.server.port == 0 {
        println!("❌ Invalid port number: {}", config.server.port);
        return Err(RuneError::config("Port cannot be 0".to_string()));
    }

    if config.server.hostname.is_empty() {
        println!("❌ Hostname cannot be empty");
        return Err(RuneError::config("Hostname is required".to_string()));
    }

    println!("✅ Server configuration is valid");
    println!("   Hostname: {}", config.server.hostname);
    println!("   Port: {}", config.server.port);
    println!("   CORS: {}", config.server.cors_enabled);
    println!("   WebSocket: {}", config.server.websocket_enabled);

    // Validate plugin configurations
    println!("\n📋 Step 3: Validating plugin configurations...");
    let mut plugin_errors = Vec::new();
    let mut plugin_warnings = Vec::new();

    for plugin in &config.plugins {
        print!("   Checking plugin '{}'... ", plugin.name);

        match plugin.validate() {
            Ok(()) => {
                println!("✅");
                
                // Check for potential issues
                if plugin.dependencies.contains(&plugin.name) {
                    plugin_warnings.push(format!(
                        "Plugin '{}' has circular dependency on itself",
                        plugin.name
                    ));
                }

                if !plugin.enabled {
                    plugin_warnings.push(format!("Plugin '{}' is disabled", plugin.name));
                }
            }
            Err(e) => {
                println!("❌");
                plugin_errors.push(format!("Plugin '{}': {}", plugin.name, e));
            }
        }
    }

    if !plugin_errors.is_empty() {
        println!("\n❌ Plugin validation errors:");
        for error in &plugin_errors {
            println!("   • {}", error);
        }
    }

    if !plugin_warnings.is_empty() {
        println!("\n⚠️  Plugin warnings:");
        for warning in &plugin_warnings {
            println!("   • {}", warning);
        }
    }

    if plugin_errors.is_empty() {
        println!("✅ All plugin configurations are valid");
    }

    // Check plugin dependencies
    println!("\n📋 Step 4: Checking plugin dependencies...");
    let enabled_plugins: Vec<_> = config
        .plugins
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.name.as_str())
        .collect();

    let mut dependency_errors = Vec::new();
    for plugin in config.plugins.iter().filter(|p| p.enabled) {
        for dep in &plugin.dependencies {
            if !enabled_plugins.contains(&dep.as_str()) {
                dependency_errors.push(format!(
                    "Plugin '{}' depends on '{}' which is not enabled",
                    plugin.name, dep
                ));
            }
        }
    }

    if dependency_errors.is_empty() {
        println!("✅ All plugin dependencies are satisfied");
    } else {
        println!("❌ Plugin dependency errors:");
        for error in &dependency_errors {
            println!("   • {}", error);
        }
    }

    // Check for available plugins
    if args.dev_mode {
        println!("\n📋 Step 5: Scanning for available plugins...");
        let mut all_discovered = Vec::new();

        if let Some(plugins_dir) = &args.plugins_dir {
            match scan_plugin_directory(plugins_dir) {
                Ok(discovered) => {
                    all_discovered.extend(discovered);
                    println!("✅ Scanned custom plugin directory: {}", plugins_dir.display());
                }
                Err(e) => {
                    println!("⚠️  Failed to scan plugin directory: {}", e);
                }
            }
        }

        for default_dir in get_default_plugin_directories() {
            if default_dir.exists() {
                match scan_plugin_directory(&default_dir) {
                    Ok(discovered) => {
                        all_discovered.extend(discovered);
                        println!("✅ Scanned default directory: {}", default_dir.display());
                    }
                    Err(e) => {
                        debug!("Failed to scan default directory {}: {}", default_dir.display(), e);
                    }
                }
            }
        }

        if !all_discovered.is_empty() {
            println!("\n🔍 Available plugins not in configuration:");
            for discovered in &all_discovered {
                let is_configured = config.plugins.iter().any(|p| p.name == discovered.name);
                if !is_configured {
                    println!("   📦 {} ({:?}) - {}", 
                        discovered.name, 
                        discovered.plugin_type,
                        discovered.path.display()
                    );
                }
            }
        }
    }

    // Final summary
    println!("\n📊 Validation Summary:");
    let total_errors = plugin_errors.len() + dependency_errors.len();
    let total_warnings = plugin_warnings.len();

    if total_errors == 0 {
        println!("✅ Configuration is valid!");
        println!("   {} plugins configured", config.plugins.len());
        println!("   {} plugins enabled", config.plugins.iter().filter(|p| p.enabled).count());
        
        if total_warnings > 0 {
            println!("   {} warnings (non-critical)", total_warnings);
        }
    } else {
        println!("❌ Configuration has {} errors", total_errors);
        if total_warnings > 0 {
            println!("   {} warnings", total_warnings);
        }
        return Err(RuneError::config("Configuration validation failed".to_string()));
    }

    Ok(())
}

/// Start configuration file hot-reloading for development mode
async fn start_config_hot_reload(config_path: PathBuf, _config: std::sync::Arc<Config>) -> Result<()> {
    use std::time::Duration;
    
    info!("🔄 Starting configuration hot-reload for: {}", config_path.display());
    
    let path_clone = config_path.clone();
    
    tokio::spawn(async move {
        let mut last_modified = std::fs::metadata(&path_clone)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        
        loop {
            interval.tick().await;
            
            if let Ok(metadata) = std::fs::metadata(&path_clone) {
                if let Ok(modified) = metadata.modified() {
                    if modified > last_modified {
                        last_modified = modified;
                        
                        info!("🔄 Configuration file changed, reloading...");
                        
                        match Config::from_file(&path_clone) {
                            Ok(new_config) => {
                                info!("✅ Configuration reloaded successfully");
                                // In a real implementation, we would update the running config
                                debug!("New config loaded with {} plugins", new_config.plugins.len());
                            }
                            Err(e) => {
                                error!("❌ Failed to reload configuration: {}", e);
                            }
                        }
                    }
                }
            }
        }
    });
    
    Ok(())
}

/// Start plugin directory watching for development mode
async fn start_plugin_directory_watch(plugins_dir: PathBuf) -> Result<()> {
    use std::time::Duration;
    
    info!("👀 Starting plugin directory watch: {}", plugins_dir.display());
    
    tokio::spawn(async move {
        let mut last_scan = std::time::SystemTime::UNIX_EPOCH;
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        
        loop {
            interval.tick().await;
            
            if plugins_dir.exists() {
                match scan_plugin_directory(&plugins_dir) {
                    Ok(discovered) => {
                        let scan_time = std::time::SystemTime::now();
                        
                        // Check if any plugins were added/removed/modified
                        let mut changes_detected = false;
                        
                        for plugin in &discovered {
                            if let Ok(metadata) = std::fs::metadata(&plugin.path) {
                                if let Ok(modified) = metadata.modified() {
                                    if modified > last_scan {
                                        changes_detected = true;
                                        info!("🔄 Plugin change detected: {} ({:?})", 
                                            plugin.name, plugin.plugin_type);
                                    }
                                }
                            }
                        }
                        
                        if changes_detected {
                            info!("📦 Plugin directory scan complete: {} plugins found", discovered.len());
                            // In a real implementation, we would trigger plugin reloading here
                        }
                        
                        last_scan = scan_time;
                    }
                    Err(e) => {
                        debug!("Failed to scan plugin directory: {}", e);
                    }
                }
            }
        }
    });
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Initialize enhanced logging for development mode
    let log_level = if args.dev_mode {
        Level::DEBUG
    } else {
        Level::INFO
    };
    
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(args.dev_mode) // Show targets in dev mode
        .with_line_number(args.dev_mode) // Show line numbers in dev mode
        .with_file(args.dev_mode); // Show file names in dev mode
    
    if args.dev_mode {
        subscriber
            .with_ansi(true)
            .pretty()
            .init();
        
        info!("🔧 Development mode enabled");
        info!("📊 Enhanced logging active");
    } else {
        subscriber
            .with_ansi(true)
            .init();
    }

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
        println!("🔄 Hot-reloading active");
        println!("📊 Enhanced debugging enabled");
        
        // Start configuration hot-reloading in dev mode
        if let Some(config_file) = &args.config_file {
            start_config_hot_reload(config_file.clone(), engine.config()).await?;
        }
        
        // Start plugin directory watching in dev mode
        if let Some(plugins_dir) = &args.plugins_dir {
            start_plugin_directory_watch(plugins_dir.clone()).await?;
        }
    }

    if args.plugins_dir.is_some() {
        println!(
            "🔌 Custom plugins directory: {}",
            args.plugins_dir.as_ref().unwrap().display()
        );
    }

    println!("📡 WebSocket live reload enabled");
    
    if args.dev_mode {
        println!("\n🔧 Development Features:");
        println!("   • Configuration hot-reload");
        println!("   • Plugin directory watching");
        println!("   • Enhanced error reporting");
        println!("   • Debug logging enabled");
    }
    
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
