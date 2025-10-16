//! File synchronization for handling external file changes during editing

use crate::EditorError;
use async_trait::async_trait;
use rune_core::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Trait for handling file synchronization between editor and file system
#[async_trait]
pub trait FileSync: Send + Sync {
    /// Detect if the file has been modified externally
    async fn detect_external_change(&self, file_path: &Path) -> Result<Option<ExternalChange>>;

    /// Resolve conflicts between local edits and external changes
    async fn resolve_conflict(
        &self,
        local_content: &str,
        external_content: &str,
        strategy: ConflictResolutionStrategy,
    ) -> Result<ConflictResolution>;

    /// Store content locally for offline editing
    async fn store_local_backup(&self, session_id: Uuid, content: &str) -> Result<()>;

    /// Retrieve locally stored content
    async fn retrieve_local_backup(&self, session_id: Uuid) -> Result<Option<String>>;

    /// Clear local backup after successful sync
    async fn clear_local_backup(&self, session_id: Uuid) -> Result<()>;

    /// Sync local changes to file system
    async fn sync_to_file(&self, file_path: &Path, content: &str) -> Result<()>;

    /// Check if local backup exists
    async fn has_local_backup(&self, session_id: Uuid) -> Result<bool>;
}

/// Information about an external file change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalChange {
    /// Path to the changed file
    pub file_path: PathBuf,
    /// New content from the file
    pub new_content: String,
    /// Timestamp of the external change
    pub timestamp: SystemTime,
    /// File modification time
    pub modified_time: SystemTime,
}

/// Strategy for resolving conflicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictResolutionStrategy {
    /// Keep local changes, discard external changes
    PreferLocal,
    /// Keep external changes, discard local changes
    PreferExternal,
    /// Attempt to merge changes automatically
    AutoMerge,
    /// Prompt user for manual resolution
    Manual,
}

/// Result of conflict resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    /// Resolved content
    pub content: String,
    /// Strategy that was used
    pub strategy_used: ConflictResolutionStrategy,
    /// Whether the resolution was successful
    pub success: bool,
    /// Conflicts that couldn't be auto-resolved
    pub unresolved_conflicts: Vec<ConflictRegion>,
}

/// A region of text with conflicting changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRegion {
    /// Start line of the conflict
    pub start_line: usize,
    /// End line of the conflict
    pub end_line: usize,
    /// Local version of the conflicting text
    pub local_version: String,
    /// External version of the conflicting text
    pub external_version: String,
}

/// File synchronization implementation
pub struct FileSyncManager {
    /// Directory for storing local backups
    backup_dir: PathBuf,
    /// File metadata cache for change detection
    file_metadata: Arc<RwLock<std::collections::HashMap<PathBuf, FileMetadata>>>,
}

/// Metadata for tracking file changes
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FileMetadata {
    /// Last known modification time
    last_modified: SystemTime,
    /// Content hash for change detection
    content_hash: String,
    /// Last sync time
    last_sync: SystemTime,
}

impl FileSyncManager {
    /// Create a new file sync manager
    pub fn new(backup_dir: PathBuf) -> Self {
        Self {
            backup_dir,
            file_metadata: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Initialize the sync manager
    pub async fn initialize(&self) -> Result<()> {
        // Create backup directory if it doesn't exist
        if !self.backup_dir.exists() {
            fs::create_dir_all(&self.backup_dir).await.map_err(|e| {
                EditorError::FileOperationFailed(format!(
                    "Failed to create backup directory: {}",
                    e
                ))
            })?;
        }

        tracing::info!(
            "File sync manager initialized with backup dir: {}",
            self.backup_dir.display()
        );
        Ok(())
    }

    /// Get backup file path for a session
    fn get_backup_path(&self, session_id: Uuid) -> PathBuf {
        self.backup_dir.join(format!("{}.backup", session_id))
    }

    /// Calculate content hash
    fn calculate_hash(content: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Update file metadata cache
    async fn update_metadata(&self, file_path: &Path, content: &str) -> Result<()> {
        let metadata = fs::metadata(file_path).await.map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to read file metadata: {}", e))
        })?;

        let modified = metadata.modified().map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to get modification time: {}", e))
        })?;

        let file_meta = FileMetadata {
            last_modified: modified,
            content_hash: Self::calculate_hash(content),
            last_sync: SystemTime::now(),
        };

        let mut cache = self.file_metadata.write().await;
        cache.insert(file_path.to_path_buf(), file_meta);

        Ok(())
    }

    /// Perform simple line-based merge
    fn simple_merge(&self, local: &str, external: &str) -> ConflictResolution {
        let local_lines: Vec<&str> = local.lines().collect();
        let external_lines: Vec<&str> = external.lines().collect();

        let mut merged_lines = Vec::new();
        let mut conflicts = Vec::new();
        let mut i = 0;
        let mut j = 0;

        while i < local_lines.len() || j < external_lines.len() {
            if i >= local_lines.len() {
                // Only external lines remain
                merged_lines.push(external_lines[j]);
                j += 1;
            } else if j >= external_lines.len() {
                // Only local lines remain
                merged_lines.push(local_lines[i]);
                i += 1;
            } else if local_lines[i] == external_lines[j] {
                // Lines match
                merged_lines.push(local_lines[i]);
                i += 1;
                j += 1;
            } else {
                // Conflict detected - for now, prefer local
                conflicts.push(ConflictRegion {
                    start_line: merged_lines.len(),
                    end_line: merged_lines.len() + 1,
                    local_version: local_lines[i].to_string(),
                    external_version: external_lines[j].to_string(),
                });
                merged_lines.push(local_lines[i]);
                i += 1;
                j += 1;
            }
        }

        ConflictResolution {
            content: merged_lines.join("\n"),
            strategy_used: ConflictResolutionStrategy::AutoMerge,
            success: conflicts.is_empty(),
            unresolved_conflicts: conflicts,
        }
    }
}

#[async_trait]
impl FileSync for FileSyncManager {
    async fn detect_external_change(&self, file_path: &Path) -> Result<Option<ExternalChange>> {
        // Check if file exists
        if !file_path.exists() {
            return Ok(None);
        }

        // Get current file metadata
        let metadata = fs::metadata(file_path).await.map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to read file metadata: {}", e))
        })?;

        let modified = metadata.modified().map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to get modification time: {}", e))
        })?;

        // Check cache for last known state
        let cache = self.file_metadata.read().await;
        let has_changed = if let Some(cached) = cache.get(file_path) {
            modified > cached.last_modified
        } else {
            // No cached data, assume changed
            true
        };

        drop(cache);

        if has_changed {
            // Read new content
            let new_content = fs::read_to_string(file_path).await.map_err(|e| {
                EditorError::FileOperationFailed(format!("Failed to read file: {}", e))
            })?;

            Ok(Some(ExternalChange {
                file_path: file_path.to_path_buf(),
                new_content,
                timestamp: SystemTime::now(),
                modified_time: modified,
            }))
        } else {
            Ok(None)
        }
    }

    async fn resolve_conflict(
        &self,
        local_content: &str,
        external_content: &str,
        strategy: ConflictResolutionStrategy,
    ) -> Result<ConflictResolution> {
        match strategy {
            ConflictResolutionStrategy::PreferLocal => Ok(ConflictResolution {
                content: local_content.to_string(),
                strategy_used: strategy,
                success: true,
                unresolved_conflicts: vec![],
            }),
            ConflictResolutionStrategy::PreferExternal => Ok(ConflictResolution {
                content: external_content.to_string(),
                strategy_used: strategy,
                success: true,
                unresolved_conflicts: vec![],
            }),
            ConflictResolutionStrategy::AutoMerge => {
                Ok(self.simple_merge(local_content, external_content))
            }
            ConflictResolutionStrategy::Manual => {
                // Return both versions for manual resolution
                Ok(ConflictResolution {
                    content: format!(
                        "<<<<<<< LOCAL\n{}\n=======\n{}\n>>>>>>> EXTERNAL",
                        local_content, external_content
                    ),
                    strategy_used: strategy,
                    success: false,
                    unresolved_conflicts: vec![ConflictRegion {
                        start_line: 0,
                        end_line: local_content.lines().count(),
                        local_version: local_content.to_string(),
                        external_version: external_content.to_string(),
                    }],
                })
            }
        }
    }

    async fn store_local_backup(&self, session_id: Uuid, content: &str) -> Result<()> {
        let backup_path = self.get_backup_path(session_id);

        fs::write(&backup_path, content).await.map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to write backup: {}", e))
        })?;

        tracing::debug!(
            "Stored local backup for session {} at {}",
            session_id,
            backup_path.display()
        );
        Ok(())
    }

    async fn retrieve_local_backup(&self, session_id: Uuid) -> Result<Option<String>> {
        let backup_path = self.get_backup_path(session_id);

        if !backup_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&backup_path).await.map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to read backup: {}", e))
        })?;

        tracing::debug!("Retrieved local backup for session {}", session_id);
        Ok(Some(content))
    }

    async fn clear_local_backup(&self, session_id: Uuid) -> Result<()> {
        let backup_path = self.get_backup_path(session_id);

        if backup_path.exists() {
            fs::remove_file(&backup_path).await.map_err(|e| {
                EditorError::FileOperationFailed(format!("Failed to remove backup: {}", e))
            })?;

            tracing::debug!("Cleared local backup for session {}", session_id);
        }

        Ok(())
    }

    async fn sync_to_file(&self, file_path: &Path, content: &str) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                EditorError::FileOperationFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        // Write content to file
        fs::write(file_path, content).await.map_err(|e| {
            EditorError::FileOperationFailed(format!("Failed to write file: {}", e))
        })?;

        // Update metadata cache
        self.update_metadata(file_path, content).await?;

        tracing::debug!("Synced content to file: {}", file_path.display());
        Ok(())
    }

    async fn has_local_backup(&self, session_id: Uuid) -> Result<bool> {
        let backup_path = self.get_backup_path(session_id);
        Ok(backup_path.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_local_backup_operations() {
        let temp_dir = tempdir().unwrap();
        let backup_dir = temp_dir.path().join("backups");
        let sync_manager = FileSyncManager::new(backup_dir);
        sync_manager.initialize().await.unwrap();

        let session_id = Uuid::new_v4();
        let content = "Test content for backup";

        // Store backup
        sync_manager
            .store_local_backup(session_id, content)
            .await
            .unwrap();

        // Check backup exists
        assert!(sync_manager.has_local_backup(session_id).await.unwrap());

        // Retrieve backup
        let retrieved = sync_manager
            .retrieve_local_backup(session_id)
            .await
            .unwrap();
        assert_eq!(retrieved, Some(content.to_string()));

        // Clear backup
        sync_manager.clear_local_backup(session_id).await.unwrap();
        assert!(!sync_manager.has_local_backup(session_id).await.unwrap());
    }

    #[tokio::test]
    async fn test_conflict_resolution_prefer_local() {
        let temp_dir = tempdir().unwrap();
        let sync_manager = FileSyncManager::new(temp_dir.path().to_path_buf());

        let local = "Local content";
        let external = "External content";

        let resolution = sync_manager
            .resolve_conflict(local, external, ConflictResolutionStrategy::PreferLocal)
            .await
            .unwrap();

        assert_eq!(resolution.content, local);
        assert!(resolution.success);
        assert!(resolution.unresolved_conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_conflict_resolution_prefer_external() {
        let temp_dir = tempdir().unwrap();
        let sync_manager = FileSyncManager::new(temp_dir.path().to_path_buf());

        let local = "Local content";
        let external = "External content";

        let resolution = sync_manager
            .resolve_conflict(local, external, ConflictResolutionStrategy::PreferExternal)
            .await
            .unwrap();

        assert_eq!(resolution.content, external);
        assert!(resolution.success);
        assert!(resolution.unresolved_conflicts.is_empty());
    }

    #[tokio::test]
    async fn test_external_change_detection() {
        let temp_dir = tempdir().unwrap();
        let file_path = temp_dir.path().join("test.md");
        let sync_manager = FileSyncManager::new(temp_dir.path().to_path_buf());
        sync_manager.initialize().await.unwrap();

        // Create initial file
        fs::write(&file_path, "Initial content").await.unwrap();

        // First detection should find change (no cache)
        let change = sync_manager
            .detect_external_change(&file_path)
            .await
            .unwrap();
        assert!(change.is_some());

        // Update metadata cache
        sync_manager
            .update_metadata(&file_path, "Initial content")
            .await
            .unwrap();

        // No change should be detected now
        let change = sync_manager
            .detect_external_change(&file_path)
            .await
            .unwrap();
        assert!(change.is_none());

        // Modify file
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        fs::write(&file_path, "Modified content").await.unwrap();

        // Change should be detected
        let change = sync_manager
            .detect_external_change(&file_path)
            .await
            .unwrap();
        assert!(change.is_some());
        assert_eq!(change.unwrap().new_content, "Modified content");
    }
}
