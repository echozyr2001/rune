//! Plugin Context and Configuration System Demo
//!
//! This example demonstrates how to use the enhanced PluginContext with:
//! - Shared resource management
//! - Plugin-specific configuration namespaces
//! - Configuration loading and validation
//! - Configuration file management

use rune_core::{
    config::{Config, PluginConfig},
    event::InMemoryEventBus,
    plugin::{
        ConfigFieldSchema, ConfigFieldType, ConfigSchema, PluginContext, PluginNamespaceConfig,
        ValidationRule,
    },
    state::StateManager,
    Result,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DatabaseConnection {
    host: String,
    port: u16,
    database: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheService {
    max_size: usize,
    ttl_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    println!("🚀 Plugin Context and Configuration System Demo");
    println!("================================================\n");

    // Create core components
    let event_bus = Arc::new(InMemoryEventBus::new());
    let config = Arc::new(create_sample_config());
    let state_manager = Arc::new(StateManager::new());

    // Create the main plugin context
    let context = PluginContext::new(event_bus, config, state_manager);

    // Demo 1: Shared Resource Management
    println!("📦 Demo 1: Shared Resource Management");
    demo_shared_resources(&context).await?;

    // Demo 2: Plugin-Specific Configuration
    println!("\n⚙️  Demo 2: Plugin-Specific Configuration");
    demo_plugin_configuration(&context).await?;

    // Demo 3: Configuration Loading and Validation
    println!("\n✅ Demo 3: Configuration Loading and Validation");
    demo_configuration_validation(&context).await?;

    // Demo 4: Configuration File Management
    println!("\n💾 Demo 4: Configuration File Management");
    demo_configuration_files(&context).await?;

    // Demo 5: Configuration Merging and Overrides
    println!("\n🔄 Demo 5: Configuration Merging and Overrides");
    demo_configuration_merging(&context).await?;

    println!("\n✨ All demos completed successfully!");
    Ok(())
}

async fn demo_shared_resources(context: &PluginContext) -> Result<()> {
    println!("  Setting up shared resources...");

    // Store a database connection that can be shared between plugins
    let db_connection = DatabaseConnection {
        host: "localhost".to_string(),
        port: 5432,
        database: "rune_db".to_string(),
    };

    context
        .set_shared_resource("database".to_string(), db_connection)
        .await?;

    // Store a cache service
    let cache_service = CacheService {
        max_size: 1000,
        ttl_seconds: 3600,
    };

    context
        .set_shared_resource("cache".to_string(), cache_service)
        .await?;

    // List available resources
    let resource_keys = context.list_shared_resource_keys().await;
    println!("  📋 Available shared resources: {:?}", resource_keys);

    // Retrieve and use a shared resource
    if let Some(db) = context
        .get_shared_resource::<DatabaseConnection>("database")
        .await
    {
        println!(
            "  🗄️  Retrieved database connection: {}:{}",
            db.host, db.port
        );
    }

    if let Some(cache) = context.get_shared_resource::<CacheService>("cache").await {
        println!(
            "  🚀 Retrieved cache service: max_size={}, ttl={}s",
            cache.max_size, cache.ttl_seconds
        );
    }

    Ok(())
}

async fn demo_plugin_configuration(context: &PluginContext) -> Result<()> {
    println!("  Creating plugin-specific contexts...");

    // Create plugin-specific contexts
    let file_watcher_context = context.for_plugin("file-watcher".to_string());
    let renderer_context = context.for_plugin("renderer".to_string());

    // Set configuration values for file-watcher plugin
    file_watcher_context
        .set_config_value("debounce_ms".to_string(), 150)
        .await?;
    file_watcher_context
        .set_config_value(
            "watch_patterns".to_string(),
            vec!["*.md", "*.txt", "*.html"],
        )
        .await?;
    file_watcher_context
        .set_config_value("recursive".to_string(), true)
        .await?;

    // Set configuration values for renderer plugin
    renderer_context
        .set_config_value("syntax_highlighting".to_string(), true)
        .await?;
    renderer_context
        .set_config_value("mermaid_enabled".to_string(), true)
        .await?;
    renderer_context
        .set_config_value("highlight_theme".to_string(), "github".to_string())
        .await?;

    // Retrieve configuration values
    let debounce: Option<u32> = file_watcher_context.get_config_value("debounce_ms").await?;
    println!("  ⏱️  File watcher debounce: {:?}ms", debounce);

    let syntax_highlighting: Option<bool> = renderer_context
        .get_config_value("syntax_highlighting")
        .await?;
    println!(
        "  🎨 Renderer syntax highlighting: {:?}",
        syntax_highlighting
    );

    // Show plugin configurations
    let file_watcher_config = file_watcher_context.get_plugin_config().await?;
    println!(
        "  📁 File watcher config keys: {:?}",
        file_watcher_config.keys()
    );

    let renderer_config = renderer_context.get_plugin_config().await?;
    println!("  🖼️  Renderer config keys: {:?}", renderer_config.keys());

    Ok(())
}

async fn demo_configuration_validation(context: &PluginContext) -> Result<()> {
    println!("  Setting up configuration schema and validation...");

    // Create a plugin context with schema
    let validator_context = context.for_plugin("validator-demo".to_string());

    // Create a configuration schema
    let mut schema = ConfigSchema::new();

    // Add field schemas with validation rules
    let mut port_field = ConfigFieldSchema::new(ConfigFieldType::Number);
    port_field.description = Some("Server port number".to_string());
    port_field.required = true;
    port_field.validation_rules.push(ValidationRule::Range {
        min: 1024.0,
        max: 65535.0,
    });
    schema.add_field("port".to_string(), port_field);

    let mut name_field = ConfigFieldSchema::new(ConfigFieldType::String);
    name_field.description = Some("Service name".to_string());
    name_field.required = true;
    name_field
        .validation_rules
        .push(ValidationRule::MinLength(3));
    name_field
        .validation_rules
        .push(ValidationRule::MaxLength(50));
    schema.add_field("name".to_string(), name_field);

    let mut enabled_field = ConfigFieldSchema::new(ConfigFieldType::Boolean);
    enabled_field.description = Some("Whether the service is enabled".to_string());
    enabled_field.required = false;
    enabled_field.recommended = true;
    schema.add_field("enabled".to_string(), enabled_field);

    schema.require_field("port".to_string());
    schema.require_field("name".to_string());

    // Create a configuration with the schema
    let mut config = PluginNamespaceConfig::new("validator-demo".to_string());
    config.schema = Some(schema);

    // Set valid configuration values
    config.set("port".to_string(), 8080)?;
    config.set("name".to_string(), "demo-service".to_string())?;
    config.set("enabled".to_string(), true)?;

    // Validate the configuration
    match config.validate() {
        Ok(_) => println!("  ✅ Configuration validation passed"),
        Err(e) => println!("  ❌ Configuration validation failed: {}", e),
    }

    // Test validation with invalid values
    let mut invalid_config = config.clone();
    invalid_config.set("port".to_string(), 80)?; // Invalid port (too low)

    match invalid_config.validate() {
        Ok(_) => println!("  ⚠️  Invalid configuration unexpectedly passed validation"),
        Err(e) => println!("  ✅ Invalid configuration correctly rejected: {}", e),
    }

    // Get validation warnings
    let warnings = config.get_validation_warnings();
    if warnings.is_empty() {
        println!("  ✅ No validation warnings");
    } else {
        println!("  ⚠️  Validation warnings: {:?}", warnings);
    }

    // Update the plugin context with the validated configuration
    validator_context.update_plugin_config(config).await?;

    Ok(())
}

async fn demo_configuration_files(context: &PluginContext) -> Result<()> {
    println!("  Demonstrating configuration file operations...");

    let file_context = context.for_plugin("file-demo".to_string());

    // Create a configuration
    let mut config = PluginNamespaceConfig::new("file-demo".to_string());
    config.set("setting1".to_string(), "value1".to_string())?;
    config.set("setting2".to_string(), 42)?;
    config.set("setting3".to_string(), vec!["a", "b", "c"])?;

    // Save to file
    let config_path = PathBuf::from("/tmp/rune_demo_config.json");
    config.save_to_file(&config_path)?;
    println!("  💾 Configuration saved to: {}", config_path.display());

    // Load from file
    let loaded_config = PluginNamespaceConfig::from_file("file-demo".to_string(), &config_path)?;
    println!("  📂 Configuration loaded from file");
    println!("     Keys: {:?}", loaded_config.keys());

    // Update the context with loaded configuration
    file_context.update_plugin_config(loaded_config).await?;

    // Create a backup
    let backup_dir = PathBuf::from("/tmp");
    config.backup(&backup_dir)?;
    println!("  🔄 Configuration backup created");

    // Clean up
    if config_path.exists() {
        std::fs::remove_file(&config_path).ok();
    }

    Ok(())
}

async fn demo_configuration_merging(context: &PluginContext) -> Result<()> {
    println!("  Demonstrating configuration merging...");

    let merge_context = context.for_plugin("merge-demo".to_string());

    // Create base configuration
    let mut base_config = PluginNamespaceConfig::new("merge-demo".to_string());
    base_config.set("base_setting".to_string(), "base_value".to_string())?;
    base_config.set("shared_setting".to_string(), "base_shared".to_string())?;
    base_config.set("number_setting".to_string(), 100)?;

    // Create override configuration
    let mut override_config = PluginNamespaceConfig::new("merge-demo".to_string());
    override_config.set("override_setting".to_string(), "override_value".to_string())?;
    override_config.set("shared_setting".to_string(), "override_shared".to_string())?;
    override_config.set("new_setting".to_string(), true)?;

    println!("  📋 Base config keys: {:?}", base_config.keys());
    println!("  📋 Override config keys: {:?}", override_config.keys());

    // Merge configurations
    base_config.merge(&override_config)?;

    println!("  🔄 After merging:");
    println!("     Keys: {:?}", base_config.keys());
    println!(
        "     shared_setting: {:?}",
        base_config.get::<String>("shared_setting")
    );
    println!(
        "     base_setting: {:?}",
        base_config.get::<String>("base_setting")
    );
    println!(
        "     override_setting: {:?}",
        base_config.get::<String>("override_setting")
    );
    println!(
        "     new_setting: {:?}",
        base_config.get::<bool>("new_setting")
    );

    // Update context
    merge_context.update_plugin_config(base_config).await?;

    // Validate all configurations in the context
    let validation_results = context.validate_all_plugin_configs().await?;
    println!("  ✅ Validation results for all plugins:");
    for result in validation_results {
        println!(
            "     {}: {} (errors: {}, warnings: {})",
            result.plugin_name,
            if result.is_valid {
                "✅ Valid"
            } else {
                "❌ Invalid"
            },
            result.errors.len(),
            result.warnings.len()
        );
    }

    Ok(())
}

fn create_sample_config() -> Config {
    let mut config = Config::new();

    // Add some sample plugin configurations
    let mut file_watcher = PluginConfig::new("file-watcher".to_string());
    file_watcher.set("debounce_ms".to_string(), 100).unwrap();
    file_watcher
        .set("watch_patterns".to_string(), vec!["*.md"])
        .unwrap();

    let mut renderer = PluginConfig::new("renderer".to_string());
    renderer
        .set("syntax_highlighting".to_string(), true)
        .unwrap();
    renderer.set("mermaid_enabled".to_string(), true).unwrap();

    config.set_plugin_config(file_watcher);
    config.set_plugin_config(renderer);

    // Add global settings
    config
        .set_global_setting("log_level".to_string(), "info".to_string())
        .unwrap();
    config
        .set_global_setting("dev_mode".to_string(), false)
        .unwrap();

    config
}
