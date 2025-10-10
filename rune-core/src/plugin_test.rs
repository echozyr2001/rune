//! Tests for the plugin system

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::config::Config;
    use crate::event::InMemoryEventBus;
    use crate::plugin::*;
    use crate::state::StateManager;
    use async_trait::async_trait;
    use std::sync::Arc;

    /// Mock plugin for testing
    struct MockPlugin {
        name: String,
        version: String,
        dependencies: Vec<String>,
        services: Vec<String>,
        status: PluginStatus,
    }

    impl MockPlugin {
        fn new(name: &str, version: &str) -> Self {
            Self {
                name: name.to_string(),
                version: version.to_string(),
                dependencies: Vec::new(),
                services: Vec::new(),
                status: PluginStatus::Active,
            }
        }

        fn with_dependencies(mut self, deps: Vec<&str>) -> Self {
            self.dependencies = deps.iter().map(|s| s.to_string()).collect();
            self
        }

        #[allow(dead_code)]
        fn with_services(mut self, services: Vec<&str>) -> Self {
            self.services = services.iter().map(|s| s.to_string()).collect();
            self
        }
    }

    #[async_trait]
    impl Plugin for MockPlugin {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            &self.version
        }

        fn dependencies(&self) -> Vec<&str> {
            self.dependencies.iter().map(|s| s.as_str()).collect()
        }

        async fn initialize(&mut self, _context: &PluginContext) -> Result<()> {
            self.status = PluginStatus::Active;
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<()> {
            self.status = PluginStatus::Stopped;
            Ok(())
        }

        fn status(&self) -> PluginStatus {
            self.status.clone()
        }

        fn provided_services(&self) -> Vec<&str> {
            self.services.iter().map(|s| s.as_str()).collect()
        }
    }

    fn create_test_context() -> PluginContext {
        let event_bus = Arc::new(InMemoryEventBus::new());
        let config = Arc::new(Config::new());
        let state_manager = Arc::new(StateManager::new());

        PluginContext::new(event_bus, config, state_manager)
    }

    #[tokio::test]
    async fn test_plugin_registry_creation() {
        let registry = PluginRegistry::new();
        assert_eq!(registry.list_plugins().len(), 0);
        assert_eq!(registry.get_system_health(), SystemHealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_plugin_registration() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        // Initialize registry
        registry.initialize(context.clone()).await.unwrap();

        // Create and register a plugin
        let plugin = Box::new(MockPlugin::new("test-plugin", "1.0.0"));
        registry.register_plugin(plugin, &context).await.unwrap();

        // Verify plugin is registered
        assert!(registry.is_plugin_loaded("test-plugin"));
        assert!(registry.is_plugin_active("test-plugin"));
        assert_eq!(registry.list_plugins().len(), 1);

        let plugin_info = registry.get_plugin_info("test-plugin").unwrap();
        assert_eq!(plugin_info.name, "test-plugin");
        assert_eq!(plugin_info.version, "1.0.0");
        assert!(matches!(plugin_info.status, PluginStatus::Active));
    }

    #[tokio::test]
    async fn test_plugin_dependency_validation() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Try to register a plugin with missing dependency
        let plugin = Box::new(
            MockPlugin::new("dependent-plugin", "1.0.0").with_dependencies(vec!["missing-plugin"]),
        );

        let result = registry.register_plugin(plugin, &context).await;
        assert!(result.is_err());
        assert!(!registry.is_plugin_loaded("dependent-plugin"));
    }

    #[tokio::test]
    async fn test_plugin_dependency_resolution() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Register base plugin first
        let base_plugin = Box::new(MockPlugin::new("base-plugin", "1.0.0"));
        registry
            .register_plugin(base_plugin, &context)
            .await
            .unwrap();

        // Register dependent plugin
        let dependent_plugin = Box::new(
            MockPlugin::new("dependent-plugin", "1.0.0").with_dependencies(vec!["base-plugin"]),
        );
        registry
            .register_plugin(dependent_plugin, &context)
            .await
            .unwrap();

        // Verify both plugins are loaded
        assert!(registry.is_plugin_loaded("base-plugin"));
        assert!(registry.is_plugin_loaded("dependent-plugin"));
        assert_eq!(registry.list_plugins().len(), 2);

        // Check dependency information
        let deps = registry.get_plugin_dependencies("dependent-plugin");
        assert_eq!(deps, vec!["base-plugin"]);

        let dependents = registry.get_dependent_plugins("base-plugin");
        assert_eq!(dependents, vec!["dependent-plugin"]);
    }

    #[tokio::test]
    async fn test_plugin_unregistration() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Register a plugin
        let plugin = Box::new(MockPlugin::new("test-plugin", "1.0.0"));
        registry.register_plugin(plugin, &context).await.unwrap();

        assert!(registry.is_plugin_loaded("test-plugin"));

        // Unregister the plugin
        registry.unregister_plugin("test-plugin").await.unwrap();

        assert!(!registry.is_plugin_loaded("test-plugin"));
        assert_eq!(registry.list_plugins().len(), 0);
    }

    #[tokio::test]
    async fn test_plugin_unregistration_with_dependents() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Register base plugin
        let base_plugin = Box::new(MockPlugin::new("base-plugin", "1.0.0"));
        registry
            .register_plugin(base_plugin, &context)
            .await
            .unwrap();

        // Register dependent plugin
        let dependent_plugin = Box::new(
            MockPlugin::new("dependent-plugin", "1.0.0").with_dependencies(vec!["base-plugin"]),
        );
        registry
            .register_plugin(dependent_plugin, &context)
            .await
            .unwrap();

        // Try to unregister base plugin - should fail
        let result = registry.unregister_plugin("base-plugin").await;
        assert!(result.is_err());
        assert!(registry.is_plugin_loaded("base-plugin"));
    }

    #[tokio::test]
    async fn test_dependency_graph_topological_sort() {
        let mut graph = DependencyGraph::new();

        // Create a dependency chain: C -> B -> A
        graph.add_dependency("plugin-c".to_string(), "plugin-b".to_string());
        graph.add_dependency("plugin-b".to_string(), "plugin-a".to_string());

        let load_order = graph.resolve_load_order().unwrap();

        // A should come before B, B should come before C
        let a_pos = load_order.iter().position(|p| p == "plugin-a").unwrap();
        let b_pos = load_order.iter().position(|p| p == "plugin-b").unwrap();
        let c_pos = load_order.iter().position(|p| p == "plugin-c").unwrap();

        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
    }

    #[tokio::test]
    async fn test_dependency_graph_circular_detection() {
        let mut graph = DependencyGraph::new();

        // Create circular dependency: A -> B -> C -> A
        graph.add_dependency("plugin-a".to_string(), "plugin-b".to_string());
        graph.add_dependency("plugin-b".to_string(), "plugin-c".to_string());
        graph.add_dependency("plugin-c".to_string(), "plugin-a".to_string());

        let result = graph.resolve_load_order();
        assert!(result.is_err());
        assert!(graph.has_circular_dependencies());
    }

    #[tokio::test]
    async fn test_plugin_health_monitoring() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Register a plugin
        let plugin = Box::new(MockPlugin::new("test-plugin", "1.0.0"));
        registry.register_plugin(plugin, &context).await.unwrap();

        // Check health status
        let health = registry.get_plugin_health("test-plugin");
        assert!(health.is_some());

        let system_health = registry.get_system_health();
        assert_eq!(system_health, SystemHealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_plugin_restart() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Register a plugin
        let plugin = Box::new(MockPlugin::new("test-plugin", "1.0.0"));
        registry.register_plugin(plugin, &context).await.unwrap();

        // Get initial restart count
        let initial_count = registry
            .get_plugin_info("test-plugin")
            .unwrap()
            .restart_count;

        // Restart the plugin
        registry.restart_plugin("test-plugin").await.unwrap();

        // Check restart count increased
        let new_count = registry
            .get_plugin_info("test-plugin")
            .unwrap()
            .restart_count;
        assert_eq!(new_count, initial_count + 1);
    }

    #[tokio::test]
    async fn test_plugin_registry_shutdown() {
        let mut registry = PluginRegistry::new();
        let context = create_test_context();

        registry.initialize(context.clone()).await.unwrap();

        // Register multiple plugins
        let plugin1 = Box::new(MockPlugin::new("plugin-1", "1.0.0"));
        let plugin2 = Box::new(MockPlugin::new("plugin-2", "1.0.0"));

        registry.register_plugin(plugin1, &context).await.unwrap();
        registry.register_plugin(plugin2, &context).await.unwrap();

        assert_eq!(registry.list_plugins().len(), 2);

        // Shutdown registry
        registry.shutdown().await.unwrap();

        // Verify all plugins are removed
        assert_eq!(registry.list_plugins().len(), 0);
        assert!(!registry.is_plugin_loaded("plugin-1"));
        assert!(!registry.is_plugin_loaded("plugin-2"));
    }
}
