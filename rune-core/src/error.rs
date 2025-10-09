//! Error handling for the Rune system

use thiserror::Error;

/// Result type alias for Rune operations
pub type Result<T> = std::result::Result<T, RuneError>;

/// Main error type for the Rune system
#[derive(Error, Debug)]
pub enum RuneError {
    /// Configuration-related errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Plugin-related errors
    #[error("Plugin error: {0}")]
    Plugin(String),

    /// Event bus errors
    #[error("Event bus error: {0}")]
    EventBus(String),

    /// File system errors
    #[error("File system error: {0}")]
    FileSystem(String),

    /// Network/server errors
    #[error("Server error: {0}")]
    Server(String),

    /// Rendering errors
    #[error("Rendering error: {0}")]
    Rendering(String),

    /// State management errors
    #[error("State error: {0}")]
    State(String),

    /// I/O errors
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization errors
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Generic errors
    #[error("Error: {0}")]
    Generic(String),
}

impl RuneError {
    /// Create a new configuration error
    pub fn config<S: Into<String>>(msg: S) -> Self {
        Self::Config(msg.into())
    }

    /// Create a new plugin error
    pub fn plugin<S: Into<String>>(msg: S) -> Self {
        Self::Plugin(msg.into())
    }

    /// Create a new event bus error
    pub fn event_bus<S: Into<String>>(msg: S) -> Self {
        Self::EventBus(msg.into())
    }

    /// Create a new file system error
    pub fn file_system<S: Into<String>>(msg: S) -> Self {
        Self::FileSystem(msg.into())
    }

    /// Create a new server error
    pub fn server<S: Into<String>>(msg: S) -> Self {
        Self::Server(msg.into())
    }

    /// Create a new rendering error
    pub fn rendering<S: Into<String>>(msg: S) -> Self {
        Self::Rendering(msg.into())
    }

    /// Create a new state error
    pub fn state<S: Into<String>>(msg: S) -> Self {
        Self::State(msg.into())
    }

    /// Create a generic error
    pub fn generic<S: Into<String>>(msg: S) -> Self {
        Self::Generic(msg.into())
    }

    /// Check if this is a recoverable error
    pub fn is_recoverable(&self) -> bool {
        match self {
            RuneError::Config(_) => false,
            RuneError::Plugin(_) => true,
            RuneError::EventBus(_) => true,
            RuneError::FileSystem(_) => true,
            RuneError::Server(_) => true,
            RuneError::Rendering(_) => true,
            RuneError::State(_) => true,
            RuneError::Io(_) => true,
            RuneError::Json(_) => false,
            RuneError::Generic(_) => true,
        }
    }

    /// Get error severity level
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            RuneError::Config(_) => ErrorSeverity::High,
            RuneError::Plugin(_) => ErrorSeverity::Medium,
            RuneError::EventBus(_) => ErrorSeverity::Medium,
            RuneError::FileSystem(_) => ErrorSeverity::Medium,
            RuneError::Server(_) => ErrorSeverity::High,
            RuneError::Rendering(_) => ErrorSeverity::Low,
            RuneError::State(_) => ErrorSeverity::Medium,
            RuneError::Io(_) => ErrorSeverity::Medium,
            RuneError::Json(_) => ErrorSeverity::Low,
            RuneError::Generic(_) => ErrorSeverity::Low,
        }
    }
}

/// Error severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorSeverity::Low => write!(f, "LOW"),
            ErrorSeverity::Medium => write!(f, "MEDIUM"),
            ErrorSeverity::High => write!(f, "HIGH"),
            ErrorSeverity::Critical => write!(f, "CRITICAL"),
        }
    }
}