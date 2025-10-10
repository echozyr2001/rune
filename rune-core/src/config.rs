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
    pub fn from_file(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RuneError::Config(format!("Failed to read config file: {}", e)))?;

        let config: Config = serde_json::from_str(&content)
            .map_err(|e| RuneError::Config(format!("Failed to parse config: {}", e)))?;

        Ok(config)
    }

    /// Save configuration to a file
    pub fn save_to_file(&self, path: &std::path::Path) -> Result<()> {
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

    /// Load configuration from multiple sources with merging
    pub fn load_with_overrides(
        base_path: &std::path::Path,
        override_paths: &[PathBuf],
    ) -> Result<Self> {
        let mut config = Self::from_file(base_path)?;

        for override_path in override_paths {
            if override_path.exists() {
                let override_config = Self::from_file(override_path)?;
                config.merge(override_config)?;
            }
        }

        Ok(config)
    }

    /// Merge another configuration into this one
    pub fn merge(&mut self, other: Config) -> Result<()> {
        // Merge server config (other takes precedence)
        if other.server.hostname != ServerConfig::default().hostname {
            self.server.hostname = other.server.hostname;
        }
        if other.server.port != ServerConfig::default().port {
            self.server.port = other.server.port;
        }
        if other.server.static_dir.is_some() {
            self.server.static_dir = other.server.static_dir;
        }
        self.server.cors_enabled = other.server.cors_enabled;
        self.server.websocket_enabled = other.server.websocket_enabled;

        // Merge plugin configurations
        for other_plugin in other.plugins {
            if let Some(existing_plugin) = self
                .plugins
                .iter_mut()
                .find(|p| p.name == other_plugin.name)
            {
                // Merge existing plugin config
                for (key, value) in other_plugin.config {
                    existing_plugin.config.insert(key, value);
                }
                existing_plugin.enabled = other_plugin.enabled;
                if other_plugin.version.is_some() {
                    existing_plugin.version = other_plugin.version;
                }
                if other_plugin.load_order.is_some() {
                    existing_plugin.load_order = other_plugin.load_order;
                }
                // Merge dependencies
                for dep in other_plugin.dependencies {
                    if !existing_plugin.dependencies.contains(&dep) {
                        existing_plugin.dependencies.push(dep);
                    }
                }
            } else {
                // Add new plugin config
                self.plugins.push(other_plugin);
            }
        }

        // Merge global settings
        for (key, value) in other.global_settings {
            self.global_settings.insert(key, value);
        }

        Ok(())
    }

    /// Get enabled plugins only
    pub fn get_enabled_plugins(&self) -> Vec<&PluginConfig> {
        self.plugins.iter().filter(|p| p.enabled).collect()
    }

    /// Get plugin configuration by name (mutable)
    pub fn get_plugin_config_mut(&mut self, name: &str) -> Option<&mut PluginConfig> {
        self.plugins.iter_mut().find(|p| p.name == name)
    }

    /// Remove plugin configuration
    pub fn remove_plugin_config(&mut self, name: &str) -> Option<PluginConfig> {
        if let Some(pos) = self.plugins.iter().position(|p| p.name == name) {
            Some(self.plugins.remove(pos))
        } else {
            None
        }
    }

    /// Get global setting value
    pub fn get_global_setting<T>(&self, key: &str) -> Option<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.global_settings
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set global setting value
    pub fn set_global_setting<T>(&mut self, key: String, value: T) -> Result<()>
    where
        T: Serialize,
    {
        let json_value = serde_json::to_value(value)
            .map_err(|e| RuneError::Config(format!("Failed to serialize global setting: {}", e)))?;

        self.global_settings.insert(key, json_value);
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
        self.config
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
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

        // Validate plugin name format (alphanumeric, hyphens, underscores)
        if !self
            .name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Err(RuneError::Config(format!(
                "Plugin name '{}' contains invalid characters. Only alphanumeric, hyphens, and underscores are allowed",
                self.name
            )));
        }

        // Validate version format if provided
        if let Some(version) = &self.version {
            if version.is_empty() {
                return Err(RuneError::Config(format!(
                    "Plugin '{}' version cannot be empty",
                    self.name
                )));
            }
        }

        // Validate dependencies don't include self
        if self.dependencies.contains(&self.name) {
            return Err(RuneError::Config(format!(
                "Plugin '{}' cannot depend on itself",
                self.name
            )));
        }

        Ok(())
    }

    /// Check if plugin has a specific dependency
    pub fn has_dependency(&self, dependency: &str) -> bool {
        self.dependencies.contains(&dependency.to_string())
    }

    /// Add a dependency if not already present
    pub fn add_dependency(&mut self, dependency: String) {
        if !self.dependencies.contains(&dependency) && dependency != self.name {
            self.dependencies.push(dependency);
        }
    }

    /// Remove a dependency
    pub fn remove_dependency(&mut self, dependency: &str) {
        self.dependencies.retain(|d| d != dependency);
    }

    /// Get configuration keys
    pub fn get_config_keys(&self) -> Vec<String> {
        self.config.keys().cloned().collect()
    }

    /// Check if configuration has a specific key
    pub fn has_config_key(&self, key: &str) -> bool {
        self.config.contains_key(key)
    }

    /// Remove a configuration key
    pub fn remove_config_key(&mut self, key: &str) -> Option<serde_json::Value> {
        self.config.remove(key)
    }

    /// Clear all configuration values
    pub fn clear_config(&mut self) {
        self.config.clear();
    }

    /// Get configuration as a pretty-printed JSON string
    pub fn config_to_string(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.config)
            .map_err(|e| RuneError::Config(format!("Failed to serialize plugin config: {}", e)))
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
