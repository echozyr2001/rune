//! Configuration management for the Rune system

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

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

    /// Create configuration with comprehensive validation
    pub fn new_with_validation() -> Result<Self> {
        let mut config = Self::new();
        config.apply_defaults()?;
        config.validate_comprehensive()?;
        Ok(config)
    }

    /// Apply default values from schema
    pub fn apply_defaults(&mut self) -> Result<()> {
        let schema = ConfigSchema::default();

        // Apply global setting defaults
        for (key, field_schema) in &schema.global_settings_schema {
            if !self.global_settings.contains_key(key) {
                if let Some(default_value) = &field_schema.default_value {
                    self.global_settings
                        .insert(key.clone(), default_value.clone());
                }
            }
        }

        Ok(())
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

    /// Comprehensive configuration validation with detailed results
    pub fn validate_comprehensive(&self) -> Result<ValidationResult> {
        let mut result = ValidationResult {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        let schema = ConfigSchema::default();

        // Validate server configuration
        self.validate_server_config(&schema.server_schema, &mut result);

        // Validate plugin configurations
        for plugin in &self.plugins {
            self.validate_plugin_config(plugin, &schema.plugin_schema, &mut result);
        }

        // Validate global settings
        self.validate_global_settings(&schema.global_settings_schema, &mut result);

        // Check for plugin dependency cycles
        self.validate_plugin_dependencies(&mut result);

        // Set overall validity
        result.is_valid = result.errors.is_empty();

        if !result.is_valid {
            return Err(RuneError::Config(format!(
                "Configuration validation failed with {} errors",
                result.errors.len()
            )));
        }

        Ok(result)
    }

    /// Validate server configuration against schema
    fn validate_server_config(&self, schema: &ServerConfigSchema, result: &mut ValidationResult) {
        // Validate hostname
        if let Err(error) = self.validate_field_value(
            "server.hostname",
            &serde_json::Value::String(self.server.hostname.clone()),
            &schema.hostname,
        ) {
            result.errors.push(error);
        }

        // Validate port
        if let Err(error) = self.validate_field_value(
            "server.port",
            &serde_json::Value::Number(serde_json::Number::from(self.server.port)),
            &schema.port,
        ) {
            result.errors.push(error);
        }

        // Validate static directory if provided
        if let Some(static_dir) = &self.server.static_dir {
            if !static_dir.exists() {
                result.warnings.push(ValidationWarning {
                    field_path: "server.static_dir".to_string(),
                    warning_type: ValidationWarningType::SuboptimalValue,
                    message: format!("Static directory does not exist: {}", static_dir.display()),
                    suggestion: Some("Create the directory or update the path".to_string()),
                });
            }
        }
    }

    /// Validate plugin configuration against schema
    fn validate_plugin_config(
        &self,
        plugin: &PluginConfig,
        schema: &PluginConfigSchema,
        result: &mut ValidationResult,
    ) {
        let base_path = format!("plugins.{}", plugin.name);

        // Validate plugin name
        if let Err(error) = self.validate_field_value(
            &format!("{}.name", base_path),
            &serde_json::Value::String(plugin.name.clone()),
            &schema.name,
        ) {
            result.errors.push(error);
        }

        // Validate version if provided
        if let Some(version) = &plugin.version {
            if let Err(error) = self.validate_field_value(
                &format!("{}.version", base_path),
                &serde_json::Value::String(version.clone()),
                &schema.version,
            ) {
                result.errors.push(error);
            }
        }

        // Validate load order if provided
        if let Some(load_order) = plugin.load_order {
            if let Err(error) = self.validate_field_value(
                &format!("{}.load_order", base_path),
                &serde_json::Value::Number(serde_json::Number::from(load_order)),
                &schema.load_order,
            ) {
                result.errors.push(error);
            }
        }

        // Check for self-dependency
        if plugin.dependencies.contains(&plugin.name) {
            result.errors.push(ValidationError {
                field_path: format!("{}.dependencies", base_path),
                error_type: ValidationErrorType::DependencyError,
                message: "Plugin cannot depend on itself".to_string(),
                suggested_fix: Some(format!("Remove '{}' from dependencies", plugin.name)),
            });
        }
    }

    /// Validate global settings against schema
    fn validate_global_settings(
        &self,
        schema: &HashMap<String, FieldSchema>,
        result: &mut ValidationResult,
    ) {
        // Check for required fields
        for (key, field_schema) in schema {
            if field_schema.required && !self.global_settings.contains_key(key) {
                result.errors.push(ValidationError {
                    field_path: format!("global_settings.{}", key),
                    error_type: ValidationErrorType::MissingRequired,
                    message: format!("Required global setting '{}' is missing", key),
                    suggested_fix: field_schema
                        .default_value
                        .as_ref()
                        .map(|v| format!("Add '{}': {}", key, v)),
                });
            }
        }

        // Validate existing settings
        for (key, value) in &self.global_settings {
            if let Some(field_schema) = schema.get(key) {
                if let Err(error) = self.validate_field_value(
                    &format!("global_settings.{}", key),
                    value,
                    field_schema,
                ) {
                    result.errors.push(error);
                }
            } else {
                result.warnings.push(ValidationWarning {
                    field_path: format!("global_settings.{}", key),
                    warning_type: ValidationWarningType::UnknownField,
                    message: format!("Unknown global setting '{}'", key),
                    suggestion: Some("Remove this setting or check for typos".to_string()),
                });
            }
        }
    }

    /// Validate plugin dependencies for cycles
    fn validate_plugin_dependencies(&self, result: &mut ValidationResult) {
        let mut visited = std::collections::HashSet::new();
        let mut rec_stack = std::collections::HashSet::new();

        for plugin in &self.plugins {
            if !visited.contains(&plugin.name)
                && self.has_dependency_cycle(&plugin.name, &mut visited, &mut rec_stack)
            {
                result.errors.push(ValidationError {
                    field_path: format!("plugins.{}.dependencies", plugin.name),
                    error_type: ValidationErrorType::CircularDependency,
                    message: format!(
                        "Circular dependency detected involving plugin '{}'",
                        plugin.name
                    ),
                    suggested_fix: Some("Review and remove circular dependencies".to_string()),
                });
            }
        }

        // Check for missing dependencies
        let plugin_names: std::collections::HashSet<_> =
            self.plugins.iter().map(|p| &p.name).collect();

        for plugin in &self.plugins {
            for dep in &plugin.dependencies {
                if !plugin_names.contains(dep) {
                    result.errors.push(ValidationError {
                        field_path: format!("plugins.{}.dependencies", plugin.name),
                        error_type: ValidationErrorType::DependencyError,
                        message: format!(
                            "Plugin '{}' depends on missing plugin '{}'",
                            plugin.name, dep
                        ),
                        suggested_fix: Some(format!(
                            "Add plugin '{}' to configuration or remove dependency",
                            dep
                        )),
                    });
                }
            }
        }
    }

    /// Check for circular dependencies using DFS
    fn has_dependency_cycle(
        &self,
        plugin_name: &str,
        visited: &mut std::collections::HashSet<String>,
        rec_stack: &mut std::collections::HashSet<String>,
    ) -> bool {
        visited.insert(plugin_name.to_string());
        rec_stack.insert(plugin_name.to_string());

        if let Some(plugin) = self.plugins.iter().find(|p| p.name == plugin_name) {
            for dep in &plugin.dependencies {
                if !visited.contains(dep) {
                    if self.has_dependency_cycle(dep, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.contains(dep) {
                    return true;
                }
            }
        }

        rec_stack.remove(plugin_name);
        false
    }

    /// Validate a single field value against its schema
    fn validate_field_value(
        &self,
        field_path: &str,
        value: &serde_json::Value,
        schema: &FieldSchema,
    ) -> std::result::Result<(), ValidationError> {
        // Check type
        let value_matches_type = matches!(
            (&schema.field_type, value),
            (FieldType::String, serde_json::Value::String(_))
                | (FieldType::Number, serde_json::Value::Number(_))
                | (FieldType::Boolean, serde_json::Value::Bool(_))
                | (FieldType::Array, serde_json::Value::Array(_))
                | (FieldType::Object, serde_json::Value::Object(_))
        );

        if !value_matches_type {
            return Err(ValidationError {
                field_path: field_path.to_string(),
                error_type: ValidationErrorType::InvalidType,
                message: format!(
                    "Expected {:?} but got {:?}",
                    schema.field_type,
                    match value {
                        serde_json::Value::String(_) => "String",
                        serde_json::Value::Number(_) => "Number",
                        serde_json::Value::Bool(_) => "Boolean",
                        serde_json::Value::Array(_) => "Array",
                        serde_json::Value::Object(_) => "Object",
                        serde_json::Value::Null => "Null",
                    }
                ),
                suggested_fix: Some(format!("Change value to {:?} type", schema.field_type)),
            });
        }

        // Apply validation rules
        for rule in &schema.validation_rules {
            match rule {
                ValidationRule::Range { min, max } => {
                    if let serde_json::Value::Number(num) = value {
                        if let Some(val) = num.as_f64() {
                            if val < *min || val > *max {
                                return Err(ValidationError {
                                    field_path: field_path.to_string(),
                                    error_type: ValidationErrorType::InvalidValue,
                                    message: format!(
                                        "Value {} is outside range [{}, {}]",
                                        val, min, max
                                    ),
                                    suggested_fix: Some(format!(
                                        "Use a value between {} and {}",
                                        min, max
                                    )),
                                });
                            }
                        }
                    }
                }
                ValidationRule::MinLength(min_len) => {
                    if let serde_json::Value::String(s) = value {
                        if s.len() < *min_len {
                            return Err(ValidationError {
                                field_path: field_path.to_string(),
                                error_type: ValidationErrorType::InvalidValue,
                                message: format!(
                                    "String length {} is less than minimum {}",
                                    s.len(),
                                    min_len
                                ),
                                suggested_fix: Some(format!(
                                    "Use a string with at least {} characters",
                                    min_len
                                )),
                            });
                        }
                    }
                }
                ValidationRule::MaxLength(max_len) => {
                    if let serde_json::Value::String(s) = value {
                        if s.len() > *max_len {
                            return Err(ValidationError {
                                field_path: field_path.to_string(),
                                error_type: ValidationErrorType::InvalidValue,
                                message: format!(
                                    "String length {} exceeds maximum {}",
                                    s.len(),
                                    max_len
                                ),
                                suggested_fix: Some(format!(
                                    "Use a string with at most {} characters",
                                    max_len
                                )),
                            });
                        }
                    }
                }
                ValidationRule::Pattern(pattern) => {
                    if let serde_json::Value::String(s) = value {
                        if let Ok(regex) = regex::Regex::new(pattern) {
                            if !regex.is_match(s) {
                                return Err(ValidationError {
                                    field_path: field_path.to_string(),
                                    error_type: ValidationErrorType::InvalidFormat,
                                    message: format!(
                                        "String '{}' does not match pattern '{}'",
                                        s, pattern
                                    ),
                                    suggested_fix: Some(
                                        "Use a string that matches the required pattern"
                                            .to_string(),
                                    ),
                                });
                            }
                        }
                    }
                }
                ValidationRule::OneOf(options) => {
                    if let serde_json::Value::String(s) = value {
                        if !options.contains(s) {
                            return Err(ValidationError {
                                field_path: field_path.to_string(),
                                error_type: ValidationErrorType::InvalidValue,
                                message: format!(
                                    "Value '{}' is not one of allowed options: {:?}",
                                    s, options
                                ),
                                suggested_fix: Some(format!("Use one of: {}", options.join(", "))),
                            });
                        }
                    }
                }
                ValidationRule::Custom(_) => {
                    // Custom validation rules would be implemented here
                    // For now, we skip them
                }
            }
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

    /// Load configuration with comprehensive context and validation
    pub fn load_with_context(context: &ConfigLoadContext) -> Result<(Self, ConfigMetadata)> {
        let start_time = SystemTime::now();
        let mut source_files = Vec::new();

        // Load base configuration
        let mut config = if context.base_path.exists() {
            source_files.push(context.base_path.clone());
            Self::from_file(&context.base_path)?
        } else {
            Self::new()
        };

        // Apply defaults
        config.apply_defaults()?;

        // Load override configurations
        for override_path in &context.override_paths {
            if override_path.exists() {
                source_files.push(override_path.clone());
                let override_config = Self::from_file(override_path)?;
                config.merge(override_config)?;
            }
        }

        // Apply environment variable overrides
        config.apply_environment_overrides(&context.environment_overrides)?;

        // Apply CLI overrides
        config.apply_cli_overrides(&context.cli_overrides)?;

        // Validate if requested
        let validation_status = if context.validation_enabled {
            match config.validate_comprehensive() {
                Ok(result) => {
                    if result.warnings.is_empty() {
                        ValidationStatus::Valid
                    } else {
                        ValidationStatus::ValidWithWarnings
                    }
                }
                Err(_) => {
                    if context.strict_mode {
                        return Err(RuneError::Config(
                            "Configuration validation failed in strict mode".to_string(),
                        ));
                    }
                    ValidationStatus::Invalid
                }
            }
        } else {
            ValidationStatus::NotValidated
        };

        // Create metadata
        let metadata = ConfigMetadata {
            version: "1.0.0".to_string(),
            created_at: start_time,
            updated_at: SystemTime::now(),
            source_files,
            checksum: config.calculate_checksum()?,
            validation_status,
        };

        Ok((config, metadata))
    }

    /// Apply environment variable overrides
    pub fn apply_environment_overrides(
        &mut self,
        env_overrides: &HashMap<String, String>,
    ) -> Result<()> {
        for (key, value) in env_overrides {
            match key.as_str() {
                "RUNE_SERVER_HOSTNAME" => self.server.hostname = value.clone(),
                "RUNE_SERVER_PORT" => {
                    self.server.port = value.parse().map_err(|_| {
                        RuneError::Config(format!(
                            "Invalid port in environment variable: {}",
                            value
                        ))
                    })?;
                }
                "RUNE_SERVER_CORS_ENABLED" => {
                    self.server.cors_enabled = value.parse().map_err(|_| {
                        RuneError::Config(format!(
                            "Invalid boolean in environment variable: {}",
                            value
                        ))
                    })?;
                }
                "RUNE_SERVER_WEBSOCKET_ENABLED" => {
                    self.server.websocket_enabled = value.parse().map_err(|_| {
                        RuneError::Config(format!(
                            "Invalid boolean in environment variable: {}",
                            value
                        ))
                    })?;
                }
                key if key.starts_with("RUNE_GLOBAL_") => {
                    let setting_key = key.strip_prefix("RUNE_GLOBAL_").unwrap().to_lowercase();
                    let json_value = if value == "true" || value == "false" {
                        serde_json::Value::Bool(value.parse().unwrap())
                    } else if let Ok(num) = value.parse::<f64>() {
                        serde_json::Value::Number(serde_json::Number::from_f64(num).unwrap())
                    } else {
                        serde_json::Value::String(value.clone())
                    };
                    self.global_settings.insert(setting_key, json_value);
                }
                _ => {
                    // Ignore unknown environment variables
                }
            }
        }
        Ok(())
    }

    /// Apply CLI argument overrides
    pub fn apply_cli_overrides(
        &mut self,
        cli_overrides: &HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        for (key, value) in cli_overrides {
            match key.as_str() {
                "server.hostname" => {
                    if let serde_json::Value::String(hostname) = value {
                        self.server.hostname = hostname.clone();
                    }
                }
                "server.port" => {
                    if let serde_json::Value::Number(port) = value {
                        if let Some(port_val) = port.as_u64() {
                            self.server.port = port_val as u16;
                        }
                    }
                }
                key if key.starts_with("global.") => {
                    let setting_key = key.strip_prefix("global.").unwrap();
                    self.global_settings
                        .insert(setting_key.to_string(), value.clone());
                }
                key if key.starts_with("plugin.") => {
                    // Handle plugin-specific overrides
                    let parts: Vec<&str> = key.splitn(3, '.').collect();
                    if parts.len() == 3 {
                        let plugin_name = parts[1];
                        let config_key = parts[2];

                        if let Some(plugin) =
                            self.plugins.iter_mut().find(|p| p.name == plugin_name)
                        {
                            plugin.config.insert(config_key.to_string(), value.clone());
                        }
                    }
                }
                _ => {
                    // Ignore unknown CLI overrides
                }
            }
        }
        Ok(())
    }

    /// Calculate configuration checksum for change detection
    pub fn calculate_checksum(&self) -> Result<String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let serialized = serde_json::to_string(self).map_err(|e| {
            RuneError::Config(format!("Failed to serialize config for checksum: {}", e))
        })?;

        let mut hasher = DefaultHasher::new();
        serialized.hash(&mut hasher);
        Ok(format!("{:x}", hasher.finish()))
    }

    /// Create a configuration diff between two configs
    pub fn diff(&self, other: &Config) -> ConfigDiff {
        let mut diff = ConfigDiff {
            server_changes: Vec::new(),
            plugin_changes: Vec::new(),
            global_setting_changes: Vec::new(),
        };

        // Compare server config
        if self.server.hostname != other.server.hostname {
            diff.server_changes.push(ConfigChange {
                field: "hostname".to_string(),
                old_value: Some(serde_json::Value::String(self.server.hostname.clone())),
                new_value: Some(serde_json::Value::String(other.server.hostname.clone())),
                change_type: ConfigChangeType::Modified,
            });
        }

        if self.server.port != other.server.port {
            diff.server_changes.push(ConfigChange {
                field: "port".to_string(),
                old_value: Some(serde_json::Value::Number(serde_json::Number::from(
                    self.server.port,
                ))),
                new_value: Some(serde_json::Value::Number(serde_json::Number::from(
                    other.server.port,
                ))),
                change_type: ConfigChangeType::Modified,
            });
        }

        // Compare plugins (simplified - could be more detailed)
        let self_plugin_names: std::collections::HashSet<_> =
            self.plugins.iter().map(|p| &p.name).collect();
        let other_plugin_names: std::collections::HashSet<_> =
            other.plugins.iter().map(|p| &p.name).collect();

        // Find added plugins
        for plugin_name in other_plugin_names.difference(&self_plugin_names) {
            diff.plugin_changes.push(ConfigChange {
                field: plugin_name.to_string(),
                old_value: None,
                new_value: Some(serde_json::Value::String("added".to_string())),
                change_type: ConfigChangeType::Added,
            });
        }

        // Find removed plugins
        for plugin_name in self_plugin_names.difference(&other_plugin_names) {
            diff.plugin_changes.push(ConfigChange {
                field: plugin_name.to_string(),
                old_value: Some(serde_json::Value::String("removed".to_string())),
                new_value: None,
                change_type: ConfigChangeType::Removed,
            });
        }

        // Compare global settings
        for (key, old_value) in &self.global_settings {
            match other.global_settings.get(key) {
                Some(new_value) if new_value != old_value => {
                    diff.global_setting_changes.push(ConfigChange {
                        field: key.clone(),
                        old_value: Some(old_value.clone()),
                        new_value: Some(new_value.clone()),
                        change_type: ConfigChangeType::Modified,
                    });
                }
                None => {
                    diff.global_setting_changes.push(ConfigChange {
                        field: key.clone(),
                        old_value: Some(old_value.clone()),
                        new_value: None,
                        change_type: ConfigChangeType::Removed,
                    });
                }
                _ => {} // No change
            }
        }

        // Find added global settings
        for (key, new_value) in &other.global_settings {
            if !self.global_settings.contains_key(key) {
                diff.global_setting_changes.push(ConfigChange {
                    field: key.clone(),
                    old_value: None,
                    new_value: Some(new_value.clone()),
                    change_type: ConfigChangeType::Added,
                });
            }
        }

        diff
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

impl Config {
    /// Get the template path from configuration or return default
    pub fn get_template_path(&self) -> Option<PathBuf> {
        self.get_global_setting::<String>("template_path")
            .map(PathBuf::from)
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

/// Configuration schema for validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSchema {
    pub version: String,
    pub server_schema: ServerConfigSchema,
    pub plugin_schema: PluginConfigSchema,
    pub global_settings_schema: HashMap<String, FieldSchema>,
}

impl Default for ConfigSchema {
    fn default() -> Self {
        Self {
            version: "1.0.0".to_string(),
            server_schema: ServerConfigSchema::default(),
            plugin_schema: PluginConfigSchema::default(),
            global_settings_schema: Self::default_global_settings_schema(),
        }
    }
}

impl ConfigSchema {
    fn default_global_settings_schema() -> HashMap<String, FieldSchema> {
        let mut schema = HashMap::new();

        schema.insert(
            "log_level".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                description: "Logging level for the application".to_string(),
                default_value: Some(serde_json::Value::String("info".to_string())),
                required: false,
                validation_rules: vec![ValidationRule::OneOf(vec![
                    "trace".to_string(),
                    "debug".to_string(),
                    "info".to_string(),
                    "warn".to_string(),
                    "error".to_string(),
                ])],
            },
        );

        schema.insert(
            "dev_mode".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                description: "Enable development mode with enhanced debugging".to_string(),
                default_value: Some(serde_json::Value::Bool(false)),
                required: false,
                validation_rules: vec![],
            },
        );

        schema.insert(
            "cache_enabled".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                description: "Enable caching for improved performance".to_string(),
                default_value: Some(serde_json::Value::Bool(true)),
                required: false,
                validation_rules: vec![],
            },
        );

        schema.insert(
            "config_auto_reload".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                description: "Automatically reload configuration when files change".to_string(),
                default_value: Some(serde_json::Value::Bool(false)),
                required: false,
                validation_rules: vec![],
            },
        );

        schema.insert(
            "plugin_hot_reload".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                description: "Enable hot reloading of plugins during development".to_string(),
                default_value: Some(serde_json::Value::Bool(false)),
                required: false,
                validation_rules: vec![],
            },
        );

        schema
    }
}

/// Server configuration schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigSchema {
    pub hostname: FieldSchema,
    pub port: FieldSchema,
    pub static_dir: FieldSchema,
    pub cors_enabled: FieldSchema,
    pub websocket_enabled: FieldSchema,
}

impl Default for ServerConfigSchema {
    fn default() -> Self {
        Self {
            hostname: FieldSchema {
                field_type: FieldType::String,
                description: "Hostname or IP address to bind the server to".to_string(),
                default_value: Some(serde_json::Value::String("127.0.0.1".to_string())),
                required: true,
                validation_rules: vec![ValidationRule::Pattern(r"^[a-zA-Z0-9.-]+$".to_string())],
            },
            port: FieldSchema {
                field_type: FieldType::Number,
                description: "Port number to bind the server to".to_string(),
                default_value: Some(serde_json::Value::Number(serde_json::Number::from(3000))),
                required: true,
                validation_rules: vec![ValidationRule::Range {
                    min: 1.0,
                    max: 65535.0,
                }],
            },
            static_dir: FieldSchema {
                field_type: FieldType::String,
                description: "Directory to serve static files from".to_string(),
                default_value: None,
                required: false,
                validation_rules: vec![],
            },
            cors_enabled: FieldSchema {
                field_type: FieldType::Boolean,
                description: "Enable Cross-Origin Resource Sharing (CORS)".to_string(),
                default_value: Some(serde_json::Value::Bool(true)),
                required: false,
                validation_rules: vec![],
            },
            websocket_enabled: FieldSchema {
                field_type: FieldType::Boolean,
                description: "Enable WebSocket support for live updates".to_string(),
                default_value: Some(serde_json::Value::Bool(true)),
                required: false,
                validation_rules: vec![],
            },
        }
    }
}

/// Plugin configuration schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfigSchema {
    pub name: FieldSchema,
    pub enabled: FieldSchema,
    pub version: FieldSchema,
    pub dependencies: FieldSchema,
    pub load_order: FieldSchema,
}

impl Default for PluginConfigSchema {
    fn default() -> Self {
        Self {
            name: FieldSchema {
                field_type: FieldType::String,
                description: "Unique name of the plugin".to_string(),
                default_value: None,
                required: true,
                validation_rules: vec![
                    ValidationRule::Pattern(r"^[a-zA-Z0-9_-]+$".to_string()),
                    ValidationRule::MinLength(1),
                    ValidationRule::MaxLength(64),
                ],
            },
            enabled: FieldSchema {
                field_type: FieldType::Boolean,
                description: "Whether the plugin is enabled".to_string(),
                default_value: Some(serde_json::Value::Bool(true)),
                required: false,
                validation_rules: vec![],
            },
            version: FieldSchema {
                field_type: FieldType::String,
                description: "Version of the plugin".to_string(),
                default_value: None,
                required: false,
                validation_rules: vec![ValidationRule::Pattern(r"^\d+\.\d+\.\d+.*$".to_string())],
            },
            dependencies: FieldSchema {
                field_type: FieldType::Array,
                description: "List of plugin dependencies".to_string(),
                default_value: Some(serde_json::Value::Array(vec![])),
                required: false,
                validation_rules: vec![],
            },
            load_order: FieldSchema {
                field_type: FieldType::Number,
                description: "Order in which to load the plugin".to_string(),
                default_value: None,
                required: false,
                validation_rules: vec![ValidationRule::Range {
                    min: 0.0,
                    max: 1000.0,
                }],
            },
        }
    }
}

/// Field schema for configuration validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldSchema {
    pub field_type: FieldType,
    pub description: String,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
    pub validation_rules: Vec<ValidationRule>,
}

/// Field types for configuration validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Array,
    Object,
}

/// Validation rules for configuration fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationRule {
    Range { min: f64, max: f64 },
    MinLength(usize),
    MaxLength(usize),
    Pattern(String),
    OneOf(Vec<String>),
    Custom(String), // Custom validation function name
}

/// Configuration validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
}

/// Configuration validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub field_path: String,
    pub error_type: ValidationErrorType,
    pub message: String,
    pub suggested_fix: Option<String>,
}

/// Configuration validation warning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub field_path: String,
    pub warning_type: ValidationWarningType,
    pub message: String,
    pub suggestion: Option<String>,
}

/// Types of validation errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationErrorType {
    MissingRequired,
    InvalidType,
    InvalidValue,
    InvalidFormat,
    DependencyError,
    CircularDependency,
}

/// Types of validation warnings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationWarningType {
    DeprecatedField,
    UnknownField,
    SuboptimalValue,
    MissingRecommended,
}

/// Configuration loading context
#[derive(Debug, Clone)]
pub struct ConfigLoadContext {
    pub base_path: PathBuf,
    pub override_paths: Vec<PathBuf>,
    pub environment_overrides: HashMap<String, String>,
    pub cli_overrides: HashMap<String, serde_json::Value>,
    pub validation_enabled: bool,
    pub strict_mode: bool,
}

impl Default for ConfigLoadContext {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("config.json"),
            override_paths: vec![],
            environment_overrides: HashMap::new(),
            cli_overrides: HashMap::new(),
            validation_enabled: true,
            strict_mode: false,
        }
    }
}

/// Configuration metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMetadata {
    pub version: String,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    pub source_files: Vec<PathBuf>,
    pub checksum: String,
    pub validation_status: ValidationStatus,
}

/// Configuration validation status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationStatus {
    Valid,
    ValidWithWarnings,
    Invalid,
    NotValidated,
}

/// Configuration difference between two configs
#[derive(Debug, Clone)]
pub struct ConfigDiff {
    pub server_changes: Vec<ConfigChange>,
    pub plugin_changes: Vec<ConfigChange>,
    pub global_setting_changes: Vec<ConfigChange>,
}

impl ConfigDiff {
    /// Check if there are any changes
    pub fn has_changes(&self) -> bool {
        !self.server_changes.is_empty()
            || !self.plugin_changes.is_empty()
            || !self.global_setting_changes.is_empty()
    }

    /// Get total number of changes
    pub fn change_count(&self) -> usize {
        self.server_changes.len() + self.plugin_changes.len() + self.global_setting_changes.len()
    }

    /// Format diff as human-readable string
    pub fn format_summary(&self) -> String {
        let mut summary = Vec::new();

        if !self.server_changes.is_empty() {
            summary.push(format!("{} server changes", self.server_changes.len()));
        }

        if !self.plugin_changes.is_empty() {
            summary.push(format!("{} plugin changes", self.plugin_changes.len()));
        }

        if !self.global_setting_changes.is_empty() {
            summary.push(format!(
                "{} global setting changes",
                self.global_setting_changes.len()
            ));
        }

        if summary.is_empty() {
            "No changes".to_string()
        } else {
            summary.join(", ")
        }
    }
}

/// Individual configuration change
#[derive(Debug, Clone)]
pub struct ConfigChange {
    pub field: String,
    pub old_value: Option<serde_json::Value>,
    pub new_value: Option<serde_json::Value>,
    pub change_type: ConfigChangeType,
}

/// Type of configuration change
#[derive(Debug, Clone)]
pub enum ConfigChangeType {
    Added,
    Modified,
    Removed,
}

/// Type alias for configuration change listeners
pub type ConfigChangeListener = Box<dyn Fn(&ConfigDiff) + Send + Sync>;

/// Runtime configuration manager for hot-reloading and validation
pub struct RuntimeConfigManager {
    current_config: Config,
    current_metadata: ConfigMetadata,
    load_context: ConfigLoadContext,
    validation_enabled: bool,
    change_listeners: Vec<ConfigChangeListener>,
}

impl RuntimeConfigManager {
    /// Create a new runtime configuration manager
    pub fn new(context: ConfigLoadContext) -> Result<Self> {
        let (config, metadata) = Config::load_with_context(&context)?;

        Ok(Self {
            current_config: config,
            current_metadata: metadata,
            load_context: context,
            validation_enabled: true,
            change_listeners: Vec::new(),
        })
    }

    /// Get current configuration
    pub fn get_config(&self) -> &Config {
        &self.current_config
    }

    /// Get current metadata
    pub fn get_metadata(&self) -> &ConfigMetadata {
        &self.current_metadata
    }

    /// Reload configuration from sources
    pub fn reload(&mut self) -> Result<ConfigDiff> {
        let (new_config, new_metadata) = Config::load_with_context(&self.load_context)?;

        let diff = self.current_config.diff(&new_config);

        if diff.has_changes() {
            self.current_config = new_config;
            self.current_metadata = new_metadata;

            // Notify listeners
            for listener in &self.change_listeners {
                listener(&diff);
            }
        }

        Ok(diff)
    }

    /// Add a change listener
    pub fn add_change_listener<F>(&mut self, listener: F)
    where
        F: Fn(&ConfigDiff) + Send + Sync + 'static,
    {
        self.change_listeners.push(Box::new(listener));
    }

    /// Validate current configuration
    pub fn validate(&self) -> Result<ValidationResult> {
        self.current_config.validate_comprehensive()
    }

    /// Update configuration with new values
    pub fn update_config(
        &mut self,
        updates: HashMap<String, serde_json::Value>,
    ) -> Result<ConfigDiff> {
        let mut new_config = self.current_config.clone();

        // Apply updates
        for (key, value) in updates {
            match key.as_str() {
                key if key.starts_with("server.") => {
                    let field = key.strip_prefix("server.").unwrap();
                    match field {
                        "hostname" => {
                            if let serde_json::Value::String(hostname) = value {
                                new_config.server.hostname = hostname;
                            }
                        }
                        "port" => {
                            if let serde_json::Value::Number(port) = value {
                                if let Some(port_val) = port.as_u64() {
                                    new_config.server.port = port_val as u16;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                key if key.starts_with("global.") => {
                    let setting_key = key.strip_prefix("global.").unwrap();
                    new_config
                        .global_settings
                        .insert(setting_key.to_string(), value);
                }
                _ => {}
            }
        }

        // Validate if enabled
        if self.validation_enabled {
            new_config.validate_comprehensive()?;
        }

        let diff = self.current_config.diff(&new_config);

        if diff.has_changes() {
            self.current_config = new_config;
            self.current_metadata.updated_at = SystemTime::now();
            self.current_metadata.checksum = self.current_config.calculate_checksum()?;

            // Notify listeners
            for listener in &self.change_listeners {
                listener(&diff);
            }
        }

        Ok(diff)
    }

    /// Check if configuration files have changed on disk
    pub fn check_for_file_changes(&self) -> Result<bool> {
        for source_file in &self.current_metadata.source_files {
            if let Ok(metadata) = std::fs::metadata(source_file) {
                if let Ok(modified) = metadata.modified() {
                    if modified > self.current_metadata.updated_at {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Auto-reload configuration if files have changed
    pub fn auto_reload_if_changed(&mut self) -> Result<Option<ConfigDiff>> {
        if self.check_for_file_changes()? {
            Ok(Some(self.reload()?))
        } else {
            Ok(None)
        }
    }

    /// Generate configuration report
    pub fn generate_report(&self) -> ConfigReport {
        let validation_result = self.validate().unwrap_or_else(|_| ValidationResult {
            is_valid: false,
            errors: vec![ValidationError {
                field_path: "unknown".to_string(),
                error_type: ValidationErrorType::InvalidValue,
                message: "Validation failed".to_string(),
                suggested_fix: None,
            }],
            warnings: vec![],
        });

        ConfigReport {
            metadata: self.current_metadata.clone(),
            validation_result,
            plugin_count: self.current_config.plugins.len(),
            enabled_plugin_count: self.current_config.get_enabled_plugins().len(),
            global_setting_count: self.current_config.global_settings.len(),
        }
    }
}

/// Configuration report for diagnostics
#[derive(Debug, Clone)]
pub struct ConfigReport {
    pub metadata: ConfigMetadata,
    pub validation_result: ValidationResult,
    pub plugin_count: usize,
    pub enabled_plugin_count: usize,
    pub global_setting_count: usize,
}

impl ConfigReport {
    /// Format report as human-readable string
    pub fn format_summary(&self) -> String {
        let status = if self.validation_result.is_valid {
            if self.validation_result.warnings.is_empty() {
                "✅ Valid"
            } else {
                "⚠️ Valid with warnings"
            }
        } else {
            "❌ Invalid"
        };

        format!(
            "Configuration Report\n\
            Status: {}\n\
            Plugins: {} total, {} enabled\n\
            Global Settings: {}\n\
            Errors: {}\n\
            Warnings: {}\n\
            Last Updated: {:?}",
            status,
            self.plugin_count,
            self.enabled_plugin_count,
            self.global_setting_count,
            self.validation_result.errors.len(),
            self.validation_result.warnings.len(),
            self.metadata.updated_at
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_creation_and_validation() {
        let config = Config::new();
        assert!(config.validate().is_ok());
        assert_eq!(config.server.hostname, "127.0.0.1");
        assert_eq!(config.server.port, 3000);
        assert!(config.plugins.is_empty());
        assert!(config.global_settings.is_empty());
    }

    #[test]
    fn test_config_with_validation() {
        let config = Config::new_with_validation().unwrap();
        let result = config.validate_comprehensive().unwrap();
        assert!(result.is_valid);
    }

    #[test]
    fn test_plugin_config_validation() {
        let mut plugin = PluginConfig::new("test-plugin".to_string());
        assert!(plugin.validate().is_ok());

        // Test invalid name
        plugin.name = "".to_string();
        assert!(plugin.validate().is_err());

        // Test self-dependency
        plugin.name = "test-plugin".to_string();
        plugin.dependencies.push("test-plugin".to_string());
        assert!(plugin.validate().is_err());
    }

    #[test]
    fn test_config_file_operations() {
        let config = Config::new();

        // Test saving to file
        let temp_file = NamedTempFile::new().unwrap();
        assert!(config.save_to_file(temp_file.path()).is_ok());

        // Test loading from file
        let loaded_config = Config::from_file(temp_file.path()).unwrap();
        assert_eq!(config.server.hostname, loaded_config.server.hostname);
        assert_eq!(config.server.port, loaded_config.server.port);
    }

    #[test]
    fn test_config_merging() {
        let mut base_config = Config::new();
        base_config.server.port = 3000;
        base_config.global_settings.insert(
            "test_setting".to_string(),
            serde_json::Value::String("base_value".to_string()),
        );

        let mut override_config = Config::new();
        override_config.server.port = 8080;
        override_config.global_settings.insert(
            "test_setting".to_string(),
            serde_json::Value::String("override_value".to_string()),
        );
        override_config
            .global_settings
            .insert("new_setting".to_string(), serde_json::Value::Bool(true));

        base_config.merge(override_config).unwrap();

        assert_eq!(base_config.server.port, 8080);
        assert_eq!(
            base_config.get_global_setting::<String>("test_setting"),
            Some("override_value".to_string())
        );
        assert_eq!(
            base_config.get_global_setting::<bool>("new_setting"),
            Some(true)
        );
    }

    #[test]
    fn test_comprehensive_validation() {
        let mut config = Config::new();

        // Add a plugin with circular dependency
        let mut plugin1 = PluginConfig::new("plugin1".to_string());
        plugin1.dependencies.push("plugin2".to_string());

        let mut plugin2 = PluginConfig::new("plugin2".to_string());
        plugin2.dependencies.push("plugin1".to_string());

        config.plugins.push(plugin1);
        config.plugins.push(plugin2);

        let result = config.validate_comprehensive();
        assert!(result.is_err()); // Should fail due to circular dependency
    }

    #[test]
    fn test_environment_overrides() {
        let mut config = Config::new();
        let mut env_overrides = HashMap::new();
        env_overrides.insert("RUNE_SERVER_PORT".to_string(), "9000".to_string());
        env_overrides.insert("RUNE_GLOBAL_dev_mode".to_string(), "true".to_string());

        config.apply_environment_overrides(&env_overrides).unwrap();

        assert_eq!(config.server.port, 9000);
        assert_eq!(config.get_global_setting::<bool>("dev_mode"), Some(true));
    }

    #[test]
    fn test_cli_overrides() {
        let mut config = Config::new();
        let mut cli_overrides = HashMap::new();
        cli_overrides.insert(
            "server.hostname".to_string(),
            serde_json::Value::String("localhost".to_string()),
        );
        cli_overrides.insert(
            "global.cache_enabled".to_string(),
            serde_json::Value::Bool(false),
        );

        config.apply_cli_overrides(&cli_overrides).unwrap();

        assert_eq!(config.server.hostname, "localhost");
        assert_eq!(
            config.get_global_setting::<bool>("cache_enabled"),
            Some(false)
        );
    }

    #[test]
    fn test_config_diff() {
        let mut config1 = Config::new();
        config1.server.port = 3000;

        let mut config2 = Config::new();
        config2.server.port = 8080;
        config2.server.hostname = "0.0.0.0".to_string();

        let diff = config1.diff(&config2);
        assert!(diff.has_changes());
        assert_eq!(diff.change_count(), 2); // port and hostname changes
    }

    #[tokio::test]
    async fn test_runtime_config_manager() {
        let temp_file = NamedTempFile::new().unwrap();
        let config = Config::new();
        config.save_to_file(temp_file.path()).unwrap();

        let context = ConfigLoadContext {
            base_path: temp_file.path().to_path_buf(),
            ..Default::default()
        };

        let mut manager = RuntimeConfigManager::new(context).unwrap();

        // Test getting current config
        let current_config = manager.get_config();
        assert_eq!(current_config.server.port, 3000);

        // Test updating config
        let mut updates = HashMap::new();
        updates.insert(
            "server.port".to_string(),
            serde_json::Value::Number(serde_json::Number::from(4000)),
        );

        let diff = manager.update_config(updates).unwrap();
        assert!(diff.has_changes());
        assert_eq!(manager.get_config().server.port, 4000);
    }

    #[test]
    fn test_validation_error_types() {
        let schema = FieldSchema {
            field_type: FieldType::Number,
            description: "Test field".to_string(),
            default_value: None,
            required: true,
            validation_rules: vec![ValidationRule::Range {
                min: 1.0,
                max: 100.0,
            }],
        };

        let config = Config::new();

        // Test invalid type
        let result = config.validate_field_value(
            "test_field",
            &serde_json::Value::String("not_a_number".to_string()),
            &schema,
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error.error_type, ValidationErrorType::InvalidType));

        // Test out of range
        let result = config.validate_field_value(
            "test_field",
            &serde_json::Value::Number(serde_json::Number::from(200)),
            &schema,
        );
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(
            error.error_type,
            ValidationErrorType::InvalidValue
        ));
    }
}
