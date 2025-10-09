//! Configuration management for the Rune system

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::error::{Result, RuneError};

/// Main system configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub plugins: Vec<PluginConfig>,
    pub global_settings: HashMap<String, serde_json::Value>,
}

impl Config {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self {
            server: ServerConfig::default(),
            plugins: Vec::new(),
            global_settings: HashMap::new(),
        }
    }

    /// Load configuration from a file
    pub fn from_file(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RuneError::Config(format!("Failed to read config file: {}", e)))?;
        
        let config: Config = serde_json::from_str(&content)
            .map_err(|e| RuneError::Config(format!("Failed to parse config: {}", e)))?;
        
        Ok(config)
    }

    /// Save configuration to a file
    pub fn save_to_file(&self, path: &PathBuf) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| RuneError::Config(format!("Failed to serialize config: {}", e)))?;
        
        std::fs::write(path, content)
            .map_err(|e| RuneError::Config(format!("Failed to write config file: {}", e)))?;
        
        Ok(())
    }

    /// Get plugin configuration by name
    pub fn get_plugin_config(&self, name: &str) -> Option<&PluginConfig> {
        self.plugins.iter().find(|p| p.name == name)
    }

    /// Add or update plugin configuration
    pub fn set_plugin_config(&mut self, config: PluginConfig) {
        if let Some(existing) = self.plugins.iter_mut().find(|p| p.name == config.name) {
            *existing = config;
        } else {
            self.plugins.push(config);
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Validate server configuration
        if self.server.port == 0 {
            return Err(RuneError::Config("Invalid port number".to_string()));
        }

        // Validate plugin configurations
        for plugin in &self.plugins {
            plugin.validate()?;
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub hostname: String,
    pub port: u16,
    pub static_dir: Option<PathBuf>,
    pub cors_enabled: bool,
    pub websocket_enabled: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            hostname: "127.0.0.1".to_string(),
            port: 3000,
            static_dir: None,
            cors_enabled: true,
            websocket_enabled: true,
        }
    }
}

/// Plugin-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub enabled: bool,
    pub version: Option<String>,
    pub config: HashMap<String, serde_json::Value>,
    pub dependencies: Vec<String>,
    pub load_order: Option<i32>,
}

impl PluginConfig {
    /// Create a new plugin configuration
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            version: None,
            config: HashMap::new(),
            dependencies: Vec::new(),
            load_order: None,
        }
    }

    /// Get a configuration value by key
    pub fn get<T>(&self, key: &str) -> Option<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.config.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set a configuration value
    pub fn set<T>(&mut self, key: String, value: T) -> Result<()>
    where
        T: Serialize,
    {
        let json_value = serde_json::to_value(value)
            .map_err(|e| RuneError::Config(format!("Failed to serialize config value: {}", e)))?;
        
        self.config.insert(key, json_value);
        Ok(())
    }

    /// Validate the plugin configuration
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(RuneError::Config("Plugin name cannot be empty".to_string()));
        }

        Ok(())
    }
}

/// System-wide configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemConfig {
    pub log_level: String,
    pub plugin_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
    pub dev_mode: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        Self {
            log_level: "info".to_string(),
            plugin_dir: None,
            config_dir: None,
            cache_dir: None,
            dev_mode: false,
        }
    }
}