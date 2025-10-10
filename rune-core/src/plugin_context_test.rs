#[cfg(test)]
mod plugin_context_tests {
    use crate::{
        config::Config,
        event::InMemoryEventBus,
        plugin::{
            ConfigFieldSchema, ConfigFieldType, ConfigSchema, PluginContext, PluginNamespaceConfig,
            ValidationRule,
        },
        state::StateManager,
    };
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestResource {
        name: String,
        value: i32,
    }

    fn create_test_context() -> PluginContext {
        let event_bus = Arc::new(InMemoryEventBus::new());
        let config = Arc::new(Config::new());
        let state_manager = Arc::new(StateManager::new());
        PluginContext::new(event_bus, config, state_manager)
    }

    #[tokio::test]
    async fn test_shared_resource_management() {
        let context = create_test_context();

        // Test setting and getting shared resources
        let resource = TestResource {
            name: "test".to_string(),
            value: 42,
        };

        context
            .set_shared_resource("test_resource".to_string(), resource.clone())
            .await
            .unwrap();

        let retrieved = context
            .get_shared_resource::<TestResource>("test_resource")
            .await;
        assert!(retrieved.is_some());
        assert_eq!(*retrieved.unwrap(), resource);

        // Test listing resource keys
        let keys = context.list_shared_resource_keys().await;
        assert!(keys.contains(&"test_resource".to_string()));

        // Test removing resources
        context
            .remove_shared_resource("test_resource")
            .await
            .unwrap();
        let retrieved_after_removal = context
            .get_shared_resource::<TestResource>("test_resource")
            .await;
        assert!(retrieved_after_removal.is_none());
    }

    #[tokio::test]
    async fn test_plugin_specific_context() {
        let context = create_test_context();

        // Test creating plugin-specific contexts
        let plugin_context = context.for_plugin("test-plugin".to_string());
        assert_eq!(plugin_context.plugin_name(), Some("test-plugin"));

        // Test that the original context doesn't have a plugin name
        assert_eq!(context.plugin_name(), None);
    }

    #[tokio::test]
    async fn test_plugin_configuration() {
        let context = create_test_context();
        let plugin_context = context.for_plugin("test-plugin".to_string());

        // Test setting and getting configuration values
        plugin_context
            .set_config_value("string_setting".to_string(), "test_value".to_string())
            .await
            .unwrap();

        plugin_context
            .set_config_value("number_setting".to_string(), 42)
            .await
            .unwrap();

        plugin_context
            .set_config_value("bool_setting".to_string(), true)
            .await
            .unwrap();

        // Retrieve values
        let string_val: Option<String> = plugin_context
            .get_config_value("string_setting")
            .await
            .unwrap();
        assert_eq!(string_val, Some("test_value".to_string()));

        let number_val: Option<i32> = plugin_context
            .get_config_value("number_setting")
            .await
            .unwrap();
        assert_eq!(number_val, Some(42));

        let bool_val: Option<bool> = plugin_context
            .get_config_value("bool_setting")
            .await
            .unwrap();
        assert_eq!(bool_val, Some(true));

        // Test getting the full configuration
        let config = plugin_context.get_plugin_config().await.unwrap();
        assert_eq!(config.namespace, "test-plugin");
        assert!(config.contains_key("string_setting"));
        assert!(config.contains_key("number_setting"));
        assert!(config.contains_key("bool_setting"));
    }

    #[tokio::test]
    async fn test_configuration_validation() {
        let context = create_test_context();
        let _plugin_context = context.for_plugin("validation-test".to_string());

        // Create a configuration with schema
        let mut config = PluginNamespaceConfig::new("validation-test".to_string());

        // Create schema with validation rules
        let mut schema = ConfigSchema::new();

        let mut port_field = ConfigFieldSchema::new(ConfigFieldType::Number);
        port_field.required = true;
        port_field.validation_rules.push(ValidationRule::Range {
            min: 1024.0,
            max: 65535.0,
        });
        schema.add_field("port".to_string(), port_field);
        schema.require_field("port".to_string());

        config.schema = Some(schema);

        // Test valid configuration
        config.set("port".to_string(), 8080).unwrap();
        assert!(config.validate().is_ok());

        // Test invalid configuration
        config.set("port".to_string(), 80).unwrap(); // Too low
        assert!(config.validate().is_err());

        // Test missing required field
        config.remove("port");
        assert!(config.validate().is_err());
    }

    #[tokio::test]
    async fn test_configuration_file_operations() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.json");

        // Create and save configuration
        let mut config = PluginNamespaceConfig::new("file-test".to_string());
        config.set("setting1".to_string(), "value1".to_string()).unwrap();
        config.set("setting2".to_string(), 42).unwrap();

        config.save_to_file(&config_path).unwrap();
        assert!(config_path.exists());

        // Load configuration from file
        let loaded_config = PluginNamespaceConfig::from_file("file-test".to_string(), &config_path).unwrap();
        assert_eq!(loaded_config.namespace, "file-test");
        assert_eq!(loaded_config.get::<String>("setting1"), Some("value1".to_string()));
        assert_eq!(loaded_config.get::<i32>("setting2"), Some(42));

        // Test backup functionality
        let backup_dir = temp_dir.path();
        config.backup(backup_dir).unwrap();

        // Check that backup file was created
        let backup_files: Vec<_> = std::fs::read_dir(backup_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_name().to_string_lossy().starts_with("file-test_backup_")
            })
            .collect();
        assert!(!backup_files.is_empty());
    }

    #[tokio::test]
    async fn test_configuration_merging() {
        // Create base configuration
        let mut base_config = PluginNamespaceConfig::new("merge-test".to_string());
        base_config.set("base_setting".to_string(), "base_value".to_string()).unwrap();
        base_config.set("shared_setting".to_string(), "base_shared".to_string()).unwrap();

        // Create override configuration
        let mut override_config = PluginNamespaceConfig::new("merge-test".to_string());
        override_config.set("override_setting".to_string(), "override_value".to_string()).unwrap();
        override_config.set("shared_setting".to_string(), "override_shared".to_string()).unwrap();

        // Merge configurations
        base_config.merge(&override_config).unwrap();

        // Verify merged results
        assert_eq!(base_config.get::<String>("base_setting"), Some("base_value".to_string()));
        assert_eq!(base_config.get::<String>("override_setting"), Some("override_value".to_string()));
        assert_eq!(base_config.get::<String>("shared_setting"), Some("override_shared".to_string()));

        // Test merging with different namespaces (should fail)
        let mut different_namespace = PluginNamespaceConfig::new("different".to_string());
        different_namespace.set("test".to_string(), "value".to_string()).unwrap();
        
        assert!(base_config.merge(&different_namespace).is_err());
    }

    #[tokio::test]
    async fn test_validation_rules() {
        // Test MinLength validation
        let min_length_rule = ValidationRule::MinLength(5);
        let short_value = serde_json::Value::String("abc".to_string());
        let long_value = serde_json::Value::String("abcdef".to_string());

        assert!(min_length_rule.validate("test_field", &short_value).is_err());
        assert!(min_length_rule.validate("test_field", &long_value).is_ok());

        // Test Range validation
        let range_rule = ValidationRule::Range { min: 10.0, max: 100.0 };
        let low_value = serde_json::Value::Number(serde_json::Number::from(5));
        let valid_value = serde_json::Value::Number(serde_json::Number::from(50));
        let high_value = serde_json::Value::Number(serde_json::Number::from(150));

        assert!(range_rule.validate("test_field", &low_value).is_err());
        assert!(range_rule.validate("test_field", &valid_value).is_ok());
        assert!(range_rule.validate("test_field", &high_value).is_err());

        // Test OneOf validation
        let allowed_values = vec![
            serde_json::Value::String("option1".to_string()),
            serde_json::Value::String("option2".to_string()),
        ];
        let one_of_rule = ValidationRule::OneOf(allowed_values);
        let valid_option = serde_json::Value::String("option1".to_string());
        let invalid_option = serde_json::Value::String("option3".to_string());

        assert!(one_of_rule.validate("test_field", &valid_option).is_ok());
        assert!(one_of_rule.validate("test_field", &invalid_option).is_err());
    }

    #[tokio::test]
    async fn test_config_field_type_matching() {
        // Test string type matching
        let string_type = ConfigFieldType::String;
        assert!(string_type.matches_value(&serde_json::Value::String("test".to_string())));
        assert!(!string_type.matches_value(&serde_json::Value::Number(serde_json::Number::from(42))));

        // Test number type matching
        let number_type = ConfigFieldType::Number;
        assert!(number_type.matches_value(&serde_json::Value::Number(serde_json::Number::from(42))));
        assert!(!number_type.matches_value(&serde_json::Value::String("test".to_string())));

        // Test boolean type matching
        let bool_type = ConfigFieldType::Boolean;
        assert!(bool_type.matches_value(&serde_json::Value::Bool(true)));
        assert!(!bool_type.matches_value(&serde_json::Value::String("test".to_string())));

        // Test array type matching
        let array_type = ConfigFieldType::Array;
        assert!(array_type.matches_value(&serde_json::Value::Array(vec![])));
        assert!(!array_type.matches_value(&serde_json::Value::String("test".to_string())));

        // Test object type matching
        let object_type = ConfigFieldType::Object;
        assert!(object_type.matches_value(&serde_json::Value::Object(serde_json::Map::new())));
        assert!(!object_type.matches_value(&serde_json::Value::String("test".to_string())));

        // Test any type matching
        let any_type = ConfigFieldType::Any;
        assert!(any_type.matches_value(&serde_json::Value::String("test".to_string())));
        assert!(any_type.matches_value(&serde_json::Value::Number(serde_json::Number::from(42))));
        assert!(any_type.matches_value(&serde_json::Value::Bool(true)));
    }

    #[tokio::test]
    async fn test_plugin_context_isolation() {
        let context = create_test_context();
        let plugin1_context = context.for_plugin("plugin1".to_string());
        let plugin2_context = context.for_plugin("plugin2".to_string());

        // Set different configurations for each plugin
        plugin1_context
            .set_config_value("setting".to_string(), "plugin1_value".to_string())
            .await
            .unwrap();

        plugin2_context
            .set_config_value("setting".to_string(), "plugin2_value".to_string())
            .await
            .unwrap();

        // Verify isolation
        let plugin1_value: Option<String> = plugin1_context
            .get_config_value("setting")
            .await
            .unwrap();
        let plugin2_value: Option<String> = plugin2_context
            .get_config_value("setting")
            .await
            .unwrap();

        assert_eq!(plugin1_value, Some("plugin1_value".to_string()));
        assert_eq!(plugin2_value, Some("plugin2_value".to_string()));

        // Verify that configurations are separate
        let plugin1_config = plugin1_context.get_plugin_config().await.unwrap();
        let plugin2_config = plugin2_context.get_plugin_config().await.unwrap();

        assert_eq!(plugin1_config.namespace, "plugin1");
        assert_eq!(plugin2_config.namespace, "plugin2");
        assert_ne!(plugin1_config.get::<String>("setting"), plugin2_config.get::<String>("setting"));
    }

    #[tokio::test]
    async fn test_configuration_validation_results() {
        let context = create_test_context();

        // Create multiple plugin contexts with different validation states
        let valid_plugin = context.for_plugin("valid-plugin".to_string());
        let invalid_plugin = context.for_plugin("invalid-plugin".to_string());

        // Set up valid configuration
        valid_plugin
            .set_config_value("valid_setting".to_string(), "valid_value".to_string())
            .await
            .unwrap();

        // Set up invalid configuration with schema
        let mut invalid_config = PluginNamespaceConfig::new("invalid-plugin".to_string());
        let mut schema = ConfigSchema::new();
        
        let mut required_field = ConfigFieldSchema::new(ConfigFieldType::String);
        required_field.required = true;
        schema.add_field("required_field".to_string(), required_field);
        schema.require_field("required_field".to_string());
        
        invalid_config.schema = Some(schema);
        // Don't set the required field, making it invalid
        
        invalid_plugin.update_plugin_config(invalid_config).await.unwrap();

        // Validate all configurations
        let validation_results = context.validate_all_plugin_configs().await.unwrap();

        // Find results for our test plugins
        let valid_result = validation_results.iter().find(|r| r.plugin_name == "valid-plugin");
        let invalid_result = validation_results.iter().find(|r| r.plugin_name == "invalid-plugin");

        assert!(valid_result.is_some());
        assert!(valid_result.unwrap().is_valid);

        assert!(invalid_result.is_some());
        assert!(!invalid_result.unwrap().is_valid);
        assert!(!invalid_result.unwrap().errors.is_empty());
    }
}