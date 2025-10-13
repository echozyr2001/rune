//! Theme management plugin for Rune

use async_trait::async_trait;
use rune_core::{Plugin, PluginContext, PluginStatus, Result, RuneError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::sync::RwLock;

/// Theme provider trait for managing themes and styling
#[async_trait]
pub trait ThemeProvider: Send + Sync {
    /// Get all available themes
    async fn available_themes(&self) -> Result<Vec<ThemeInfo>>;

    /// Load a specific theme by name
    async fn load_theme(&self, name: &str) -> Result<Theme>;

    /// Get the current active theme
    async fn get_current_theme(&self) -> Result<Option<String>>;

    /// Set the active theme
    async fn set_current_theme(&self, name: &str) -> Result<()>;

    /// Watch for theme changes (returns a receiver for theme change events)
    async fn watch_theme_changes(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<ThemeChangeEvent>>;

    /// Load theme from file system
    async fn load_theme_from_file(&self, path: &Path) -> Result<Theme>;

    /// Save theme to file system
    async fn save_theme_to_file(&self, theme: &Theme, path: &Path) -> Result<()>;

    /// Validate theme structure and content
    async fn validate_theme(&self, theme: &Theme) -> Result<ThemeValidationResult>;
}

/// Theme information metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeInfo {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub icon: Option<String>,
    pub preview_colors: Vec<String>,
    pub is_dark: bool,
    pub created_at: SystemTime,
    pub modified_at: SystemTime,
}

/// Complete theme definition with all assets and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub info: ThemeInfo,
    pub css: String,
    pub javascript: Option<String>,
    pub assets: HashMap<String, Vec<u8>>,
    pub variables: HashMap<String, String>,
    pub mermaid_theme: Option<String>,
}

impl Theme {
    /// Create a new theme with basic information
    pub fn new(name: String, css: String) -> Self {
        Self {
            info: ThemeInfo {
                name: name.clone(),
                display_name: name.clone(),
                description: String::new(),
                author: "Unknown".to_string(),
                version: "1.0.0".to_string(),
                icon: None,
                preview_colors: Vec::new(),
                is_dark: false,
                created_at: SystemTime::now(),
                modified_at: SystemTime::now(),
            },
            css,
            javascript: None,
            assets: HashMap::new(),
            variables: HashMap::new(),
            mermaid_theme: None,
        }
    }

    /// Get theme variable value
    pub fn get_variable(&self, key: &str) -> Option<&String> {
        self.variables.get(key)
    }

    /// Set theme variable
    pub fn set_variable(&mut self, key: String, value: String) {
        self.variables.insert(key, value);
        self.info.modified_at = SystemTime::now();
    }

    /// Add asset to theme
    pub fn add_asset(&mut self, name: String, data: Vec<u8>) {
        self.assets.insert(name, data);
        self.info.modified_at = SystemTime::now();
    }

    /// Get asset from theme
    pub fn get_asset(&self, name: &str) -> Option<&Vec<u8>> {
        self.assets.get(name)
    }

    /// Update theme metadata
    pub fn update_info(&mut self, info: ThemeInfo) {
        self.info = info;
        self.info.modified_at = SystemTime::now();
    }
}

/// Theme change event for notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeChangeEvent {
    pub event_type: ThemeChangeType,
    pub theme_name: String,
    pub timestamp: SystemTime,
}

/// Types of theme change events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThemeChangeType {
    ThemeActivated,
    ThemeLoaded,
    ThemeUnloaded,
    ThemeModified,
    ThemeDeleted,
}

/// Theme validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Default theme provider implementation
pub struct DefaultThemeProvider {
    themes: RwLock<HashMap<String, Theme>>,
    current_theme: RwLock<Option<String>>,
    theme_change_sender: tokio::sync::broadcast::Sender<ThemeChangeEvent>,
    template_path: Option<PathBuf>,
}

impl DefaultThemeProvider {
    /// Create a new default theme provider
    pub fn new() -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(100);

        Self {
            themes: RwLock::new(HashMap::new()),
            current_theme: RwLock::new(None),
            theme_change_sender: sender,
            template_path: None,
        }
    }

    /// Create theme provider with template path
    pub fn with_template_path(template_path: PathBuf) -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(100);

        Self {
            themes: RwLock::new(HashMap::new()),
            current_theme: RwLock::new(None),
            theme_change_sender: sender,
            template_path: Some(template_path),
        }
    }

    /// Load built-in themes from template system
    async fn load_builtin_themes(&self) -> Result<()> {
        let mut themes = self.themes.write().await;

        // Extract themes from the existing template.html
        let builtin_themes = self.extract_builtin_themes().await?;

        for theme in builtin_themes {
            themes.insert(theme.info.name.clone(), theme);
        }

        // Set default theme if none is set
        let mut current = self.current_theme.write().await;
        if current.is_none() && !themes.is_empty() {
            *current = Some("catppuccin-mocha".to_string());
        }

        Ok(())
    }

    /// Extract built-in themes from template system
    async fn extract_builtin_themes(&self) -> Result<Vec<Theme>> {
        let mut themes = Vec::new();

        // Try to read from template file if path is provided
        if let Some(template_path) = &self.template_path {
            if template_path.exists() {
                tracing::debug!("Loading themes from template file: {:?}", template_path);
                // In a real implementation, we would parse the template.html file
                // to extract theme definitions. For now, we'll use the hardcoded definitions.
            }
        }

        // Define built-in themes based on the existing template.html
        let theme_definitions = vec![
            (
                "light",
                "Light",
                "☀️",
                vec!["#fff", "#333", "#0366d6"],
                false,
            ),
            (
                "dark",
                "Dark",
                "🌙",
                vec!["#0d1117", "#e6edf3", "#58a6ff"],
                true,
            ),
            (
                "catppuccin-latte",
                "Catppuccin Latte",
                "☕",
                vec!["#eff1f5", "#4c4f69", "#1e66f5"],
                false,
            ),
            (
                "catppuccin-macchiato",
                "Catppuccin Macchiato",
                "🥛",
                vec!["#24273a", "#cad3f5", "#8aadf4"],
                true,
            ),
            (
                "catppuccin-mocha",
                "Catppuccin Mocha",
                "🐱",
                vec!["#1e1e2e", "#cdd6f4", "#89b4fa"],
                true,
            ),
        ];

        for (name, display_name, icon, colors, is_dark) in theme_definitions {
            let css = self.generate_theme_css(name).await?;
            let mut theme = Theme::new(name.to_string(), css);

            theme.info.display_name = display_name.to_string();
            theme.info.description = format!("{} theme for markdown preview", display_name);
            theme.info.author = "Rune".to_string();
            theme.info.icon = Some(icon.to_string());
            theme.info.preview_colors = colors.iter().map(|s| s.to_string()).collect();
            theme.info.is_dark = is_dark;

            // Set Mermaid theme mapping
            theme.mermaid_theme = Some(if is_dark {
                "dark".to_string()
            } else {
                "default".to_string()
            });

            themes.push(theme);
        }

        Ok(themes)
    }

    /// Generate CSS for a specific theme
    async fn generate_theme_css(&self, theme_name: &str) -> Result<String> {
        // This would extract the CSS variables for the specific theme
        // For now, return a basic CSS structure
        let css = match theme_name {
            "light" => {
                r#"
                :root {
                    --bg-color: #fff;
                    --text-color: #333;
                    --border-color: #eaecef;
                    --border-color-light: #dfe2e5;
                    --code-bg: #f6f8fa;
                    --blockquote-color: #6a737d;
                    --link-color: #0366d6;
                    --table-header-bg: #f6f8fa;
                }
            "#
            }
            "dark" => {
                r#"
                :root {
                    --bg-color: #0d1117;
                    --text-color: #e6edf3;
                    --border-color: #30363d;
                    --border-color-light: #21262d;
                    --code-bg: #161b22;
                    --blockquote-color: #8b949e;
                    --link-color: #58a6ff;
                    --table-header-bg: #161b22;
                }
            "#
            }
            "catppuccin-latte" => {
                r#"
                :root {
                    --bg-color: #eff1f5;
                    --text-color: #4c4f69;
                    --border-color: #bcc0cc;
                    --border-color-light: #ccd0da;
                    --code-bg: #e6e9ef;
                    --blockquote-color: #6c6f85;
                    --link-color: #1e66f5;
                    --table-header-bg: #ccd0da;
                }
            "#
            }
            "catppuccin-macchiato" => {
                r#"
                :root {
                    --bg-color: #24273a;
                    --text-color: #cad3f5;
                    --border-color: #494d64;
                    --border-color-light: #363a4f;
                    --code-bg: #1e2030;
                    --blockquote-color: #a5adcb;
                    --link-color: #8aadf4;
                    --table-header-bg: #363a4f;
                }
            "#
            }
            "catppuccin-mocha" => {
                r#"
                :root {
                    --bg-color: #1e1e2e;
                    --text-color: #cdd6f4;
                    --border-color: #45475a;
                    --border-color-light: #313244;
                    --code-bg: #181825;
                    --blockquote-color: #a6adc8;
                    --link-color: #89b4fa;
                    --table-header-bg: #313244;
                }
            "#
            }
            _ => return Err(RuneError::theme(format!("Unknown theme: {}", theme_name))),
        };

        Ok(css.to_string())
    }

    /// Notify theme change
    async fn notify_theme_change(&self, event_type: ThemeChangeType, theme_name: String) {
        let event = ThemeChangeEvent {
            event_type,
            theme_name,
            timestamp: SystemTime::now(),
        };

        if let Err(e) = self.theme_change_sender.send(event) {
            tracing::warn!("Failed to send theme change notification: {}", e);
        }
    }
}

impl Default for DefaultThemeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ThemeProvider for DefaultThemeProvider {
    async fn available_themes(&self) -> Result<Vec<ThemeInfo>> {
        let themes = self.themes.read().await;
        Ok(themes.values().map(|theme| theme.info.clone()).collect())
    }

    async fn load_theme(&self, name: &str) -> Result<Theme> {
        let themes = self.themes.read().await;
        themes
            .get(name)
            .cloned()
            .ok_or_else(|| RuneError::theme(format!("Theme not found: {}", name)))
    }

    async fn get_current_theme(&self) -> Result<Option<String>> {
        let current = self.current_theme.read().await;
        Ok(current.clone())
    }

    async fn set_current_theme(&self, name: &str) -> Result<()> {
        // Verify theme exists
        {
            let themes = self.themes.read().await;
            if !themes.contains_key(name) {
                return Err(RuneError::theme(format!("Theme not found: {}", name)));
            }
        }

        // Set current theme
        {
            let mut current = self.current_theme.write().await;
            *current = Some(name.to_string());
        }

        // Notify change
        self.notify_theme_change(ThemeChangeType::ThemeActivated, name.to_string())
            .await;

        Ok(())
    }

    async fn watch_theme_changes(
        &self,
    ) -> Result<tokio::sync::broadcast::Receiver<ThemeChangeEvent>> {
        Ok(self.theme_change_sender.subscribe())
    }

    async fn load_theme_from_file(&self, path: &Path) -> Result<Theme> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| RuneError::theme(format!("Failed to read theme file: {}", e)))?;

        let theme: Theme = serde_json::from_str(&content)
            .map_err(|e| RuneError::theme(format!("Failed to parse theme file: {}", e)))?;

        // Add to themes collection
        {
            let mut themes = self.themes.write().await;
            themes.insert(theme.info.name.clone(), theme.clone());
        }

        // Notify theme loaded
        self.notify_theme_change(ThemeChangeType::ThemeLoaded, theme.info.name.clone())
            .await;

        Ok(theme)
    }

    async fn save_theme_to_file(&self, theme: &Theme, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(theme)
            .map_err(|e| RuneError::theme(format!("Failed to serialize theme: {}", e)))?;

        tokio::fs::write(path, content)
            .await
            .map_err(|e| RuneError::theme(format!("Failed to write theme file: {}", e)))?;

        Ok(())
    }

    async fn validate_theme(&self, theme: &Theme) -> Result<ThemeValidationResult> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        // Validate theme name
        if theme.info.name.is_empty() {
            errors.push("Theme name cannot be empty".to_string());
        }

        // Validate CSS
        if theme.css.is_empty() {
            errors.push("Theme CSS cannot be empty".to_string());
        }

        // Check for required CSS variables
        let required_vars = vec![
            "--bg-color",
            "--text-color",
            "--border-color",
            "--code-bg",
            "--link-color",
        ];

        for var in required_vars {
            if !theme.css.contains(var) {
                warnings.push(format!("Missing recommended CSS variable: {}", var));
            }
        }

        // Validate version format
        if !theme.info.version.contains('.') {
            warnings.push("Version should follow semantic versioning (e.g., 1.0.0)".to_string());
        }

        Ok(ThemeValidationResult {
            is_valid: errors.is_empty(),
            errors,
            warnings,
        })
    }
}

/// Theme management plugin implementation
pub struct ThemePlugin {
    name: String,
    version: String,
    status: PluginStatus,
    theme_provider: Option<Box<dyn ThemeProvider>>,
}

impl ThemePlugin {
    /// Create a new theme plugin
    pub fn new() -> Self {
        Self {
            name: "theme".to_string(),
            version: "0.1.0".to_string(),
            status: PluginStatus::Loading,
            theme_provider: None,
        }
    }

    /// Get the theme provider
    pub fn theme_provider(&self) -> Option<&dyn ThemeProvider> {
        self.theme_provider.as_ref().map(|p| p.as_ref())
    }
}

impl Default for ThemePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for ThemePlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn dependencies(&self) -> Vec<&str> {
        vec![] // No dependencies for theme management
    }

    async fn initialize(&mut self, context: &PluginContext) -> Result<()> {
        tracing::info!("Initializing theme plugin");

        // Create theme provider
        let template_path = context
            .config
            .get_template_path()
            .unwrap_or_else(|| PathBuf::from("template.html"));

        let provider = DefaultThemeProvider::with_template_path(template_path);

        // Load built-in themes
        provider.load_builtin_themes().await?;

        self.theme_provider = Some(Box::new(provider));
        self.status = PluginStatus::Active;

        tracing::info!("Theme plugin initialized successfully");
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down theme plugin");
        self.theme_provider = None;
        Ok(())
    }

    fn status(&self) -> PluginStatus {
        self.status.clone()
    }

    fn provided_services(&self) -> Vec<&str> {
        vec!["theme-management", "css-serving", "theme-provider"]
    }
}
