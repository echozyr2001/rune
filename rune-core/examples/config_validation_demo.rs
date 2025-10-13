//! Configuration validation and loading demonstration

use rune_core::{Config, ConfigLoadContext, PluginConfig, RuntimeConfigManager, Result, ServerConfig};
use std::collections::HashMap;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🔧 Configuration Management Demo\n");

    // Demo 1: Basic configuration loading and validation
    demo_basic_validation().await?;

    // Demo 2: Configuration loading with overrides
    demo_configuration_overrides().await?;

    // Demo 3: Runtime configuration management
    demo_runtime_management().await?;

    // Demo 4: Configuration validation with errors
    demo_validation_errors().await?;

    println!("✅ All configuration demos completed successfully!");
    Ok(())
}

/// Demonstrate basic configuration validation
async fn demo_basic_validation() -> Result<()> {
    println!("📋 Demo 1: Basic Configuration Validation");

    let config_path = PathBuf::from("rune-core/examples/config/comprehensive-config.json");
    
    if !config_path.exists() {
        println!("⚠️  Configuration file not found, creating default config");
        let config = Config::new_with_validation()?;
        config.save_to_file(&config_path)?;
    }

    // Load and validate configuration
    let config = Config::from_file(&config_path)?;
    println!("✅ Configuration loaded successfully");

    // Perform comprehensive validation
    match config.validate_comprehensive() {
        Ok(result) => {
            println!("✅ Configuration is valid");
            if !result.warnings.is_empty() {
                println!("⚠️  {} warnings found:", result.warnings.len());
                for warning in &result.warnings {
                    println!("   - {}: {}", warning.field_path, warning.message);
                }
            }
        }
        Err(e) => {
            println!("❌ Configuration validation failed: {}", e);
        }
    }

    println!("   Server: {}:{}", config.server.hostname, config.server.port);
    println!("   Plugins: {} configured, {} enabled", 
             config.plugins.len(), 
             config.get_enabled_plugins().len());
    println!("   Global settings: {}", config.global_settings.len());
    println!();

    Ok(())
}

/// Demonstrate configuration loading with overrides
async fn demo_configuration_overrides() -> Result<()> {
    println!("🔄 Demo 2: Configuration Loading with Overrides");

    let base_config_path = PathBuf::from("rune-core/examples/config/main.json");
    
    // Create override configuration
    let override_config = Config {
        server: ServerConfig {
            hostname: "0.0.0.0".to_string(),
            port: 8080,
            cors_enabled: false,
            websocket_enabled: true,
            static_dir: Some(PathBuf::from("./public")),
        },
        plugins: vec![],
        global_settings: {
            let mut settings = HashMap::new();
            settings.insert("dev_mode".to_string(), serde_json::Value::Bool(true));
            settings.insert("log_level".to_string(), serde_json::Value::String("debug".to_string()));
            settings
        },
    };

    let override_path = PathBuf::from("rune-core/examples/config/override.json");
    override_config.save_to_file(&override_path)?;

    // Load with overrides
    let merged_config = Config::load_with_overrides(&base_config_path, &[override_path.clone()])?;

    println!("✅ Configuration loaded with overrides");
    println!("   Base config: {}", base_config_path.display());
    println!("   Override config: {}", override_path.display());
    println!("   Final server: {}:{}", merged_config.server.hostname, merged_config.server.port);
    println!("   CORS enabled: {}", merged_config.server.cors_enabled);
    
    if let Some(dev_mode) = merged_config.get_global_setting::<bool>("dev_mode") {
        println!("   Dev mode: {}", dev_mode);
    }

    // Clean up
    let _ = std::fs::remove_file(override_path);
    println!();

    Ok(())
}

/// Demonstrate runtime configuration management
async fn demo_runtime_management() -> Result<()> {
    println!("⚡ Demo 3: Runtime Configuration Management");

    let context = ConfigLoadContext {
        base_path: PathBuf::from("rune-core/examples/config/main.json"),
        override_paths: vec![],
        environment_overrides: {
            let mut env = HashMap::new();
            env.insert("RUNE_SERVER_PORT".to_string(), "9000".to_string());
            env.insert("RUNE_GLOBAL_dev_mode".to_string(), "true".to_string());
            env
        },
        cli_overrides: {
            let mut cli = HashMap::new();
            cli.insert("server.hostname".to_string(), serde_json::Value::String("localhost".to_string()));
            cli
        },
        validation_enabled: true,
        strict_mode: false,
    };

    let mut manager = RuntimeConfigManager::new(context)?;
    println!("✅ Runtime configuration manager created");

    // Add a change listener
    manager.add_change_listener(|diff| {
        println!("🔄 Configuration changed: {}", diff.format_summary());
    });

    // Get current configuration
    let config = manager.get_config();
    println!("   Current server: {}:{}", config.server.hostname, config.server.port);

    // Update configuration
    let mut updates = HashMap::new();
    updates.insert("server.port".to_string(), serde_json::Value::Number(serde_json::Number::from(4000)));
    updates.insert("global.cache_enabled".to_string(), serde_json::Value::Bool(false));

    let diff = manager.update_config(updates)?;
    println!("✅ Configuration updated");
    println!("   Changes: {}", diff.format_summary());

    // Generate report
    let report = manager.generate_report();
    println!("📊 Configuration Report:");
    println!("{}", report.format_summary());
    println!();

    Ok(())
}

/// Demonstrate configuration validation with errors
async fn demo_validation_errors() -> Result<()> {
    println!("❌ Demo 4: Configuration Validation with Errors");

    // Create an invalid configuration
    let invalid_config = Config {
        server: ServerConfig {
            hostname: "".to_string(), // Invalid: empty hostname
            port: 0, // Invalid: port 0
            cors_enabled: true,
            websocket_enabled: true,
            static_dir: None,
        },
        plugins: vec![
            PluginConfig {
                name: "".to_string(), // Invalid: empty name
                enabled: true,
                version: Some("invalid-version".to_string()), // Invalid: doesn't match semver pattern
                config: HashMap::new(),
                dependencies: vec!["self".to_string()], // Invalid: self-dependency (will be caught by name validation)
                load_order: Some(-1), // Invalid: negative load order
            },
            PluginConfig {
                name: "plugin2".to_string(),
                enabled: true,
                version: None,
                config: HashMap::new(),
                dependencies: vec!["missing-plugin".to_string()], // Invalid: missing dependency
                load_order: None,
            },
        ],
        global_settings: {
            let mut settings = HashMap::new();
            settings.insert("log_level".to_string(), serde_json::Value::String("invalid".to_string())); // Invalid: not in allowed values
            settings.insert("unknown_setting".to_string(), serde_json::Value::String("value".to_string())); // Warning: unknown setting
            settings
        },
    };

    println!("🔍 Validating intentionally invalid configuration...");

    match invalid_config.validate_comprehensive() {
        Ok(result) => {
            println!("⚠️  Configuration passed validation but has warnings:");
            for warning in &result.warnings {
                println!("   Warning - {}: {}", warning.field_path, warning.message);
                if let Some(suggestion) = &warning.suggestion {
                    println!("     Suggestion: {}", suggestion);
                }
            }
        }
        Err(_) => {
            // Get detailed validation results
            if let Ok(result) = invalid_config.validate_comprehensive() {
                println!("❌ Configuration validation failed with {} errors:", result.errors.len());
                
                for error in &result.errors {
                    println!("   Error - {}: {}", error.field_path, error.message);
                    if let Some(fix) = &error.suggested_fix {
                        println!("     Suggested fix: {}", fix);
                    }
                }

                if !result.warnings.is_empty() {
                    println!("\n⚠️  Also found {} warnings:", result.warnings.len());
                    for warning in &result.warnings {
                        println!("   Warning - {}: {}", warning.field_path, warning.message);
                    }
                }
            }
        }
    }

    println!("✅ Validation error demonstration completed");
    println!();

    Ok(())
}