#[cfg(test)]
mod tests {
    use crate::event::{serialization, ChangeType, ClientInfo, ErrorSeverity, Event, SystemEvent};
    use std::path::PathBuf;
    use std::time::SystemTime;
    use uuid::Uuid;

    #[test]
    fn test_event_creation_helpers() {
        let file_event = SystemEvent::file_changed(PathBuf::from("test.md"), ChangeType::Modified);

        assert_eq!(file_event.event_type(), "file_changed");
        assert!(file_event.is_file_event());
        assert!(!file_event.is_error());

        let client_info = ClientInfo {
            user_agent: Some("Mozilla/5.0".to_string()),
            ip_address: "127.0.0.1".to_string(),
            connected_at: SystemTime::now(),
        };

        let client_event = SystemEvent::client_connected(Uuid::new_v4(), client_info);
        assert_eq!(client_event.event_type(), "client_connected");
        assert!(client_event.is_client_event());

        let error_event = SystemEvent::error(
            "test".to_string(),
            "Test error".to_string(),
            ErrorSeverity::High,
        );
        assert_eq!(error_event.event_type(), "error");
        assert!(error_event.is_error());
    }

    #[test]
    fn test_event_metadata() {
        let file_event =
            SystemEvent::file_changed(PathBuf::from("/path/to/test.md"), ChangeType::Created);

        let metadata = file_event.metadata();
        assert_eq!(metadata.get("path"), Some(&"/path/to/test.md".to_string()));
        assert_eq!(metadata.get("change_type"), Some(&"Created".to_string()));
    }

    #[test]
    fn test_event_serialization() {
        let event = SystemEvent::error(
            "test_source".to_string(),
            "Test message".to_string(),
            ErrorSeverity::Medium,
        );

        // Test serialization
        let json = serialization::serialize_event(&event).unwrap();
        assert!(json.contains("test_source"));
        assert!(json.contains("Test message"));
        assert!(json.contains("Medium"));

        // Test deserialization
        let deserialized = serialization::deserialize_event(&json).unwrap();
        assert_eq!(deserialized.event_type(), event.event_type());
        assert_eq!(deserialized.description(), event.description());
    }

    #[test]
    fn test_event_batch_serialization() {
        let events = vec![
            SystemEvent::file_changed(PathBuf::from("test1.md"), ChangeType::Created),
            SystemEvent::file_changed(PathBuf::from("test2.md"), ChangeType::Modified),
        ];

        let json = serialization::serialize_event_batch(&events).unwrap();
        let deserialized = serialization::deserialize_event_batch(&json).unwrap();

        assert_eq!(deserialized.len(), 2);
        assert_eq!(deserialized[0].event_type(), "file_changed");
        assert_eq!(deserialized[1].event_type(), "file_changed");
    }

    #[test]
    fn test_event_debug_formatting() {
        let event = SystemEvent::theme_changed("dark".to_string());

        let log_format = serialization::format_event_for_log(&event);
        assert!(log_format.contains("THEME_CHANGED"));

        let debug_string = serialization::event_debug_string(&event);
        assert!(debug_string.contains("Theme changed to dark"));
    }
}
