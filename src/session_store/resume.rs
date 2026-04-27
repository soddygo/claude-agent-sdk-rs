//! Session resume materialization
//!
//! This module provides functionality to materialize a session from a SessionStore
//! into a temporary location that Claude Code CLI can use for resume functionality.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::errors::ClaudeError;
use crate::Result;
use crate::session_store::disk::{project_key_for_directory, session_path, sessions_dir};
use crate::types::session_store::{SessionKey, SessionStore};

/// Materialized resume session
///
/// This represents a session that has been materialized from a SessionStore
/// into a temporary location for use with Claude Code CLI resume.
///
/// Note: The temporary directory is automatically cleaned up when this struct is dropped.
pub struct MaterializedResume {
    /// The config directory containing the materialized session
    pub config_dir: PathBuf,
    /// The session ID for the resumed session
    pub resume_session_id: String,
    /// Keep temp_dir alive - it handles cleanup when dropped
    #[allow(dead_code)]
    temp_dir: tempfile::TempDir,
}

impl std::fmt::Debug for MaterializedResume {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterializedResume")
            .field("config_dir", &self.config_dir)
            .field("resume_session_id", &self.resume_session_id)
            .finish()
    }
}

impl MaterializedResume {
    /// Get the path to the resumed session file
    pub fn session_path(&self) -> PathBuf {
        session_path(&self.config_dir, &self.resume_session_id)
    }
}

/// Materialize a session from a SessionStore to a temporary location
///
/// This function loads a session from the provided SessionStore and writes it
/// to a temporary directory that can be used with Claude Code CLI's resume
/// functionality.
///
/// # Arguments
///
/// * `store` - The SessionStore to load from
/// * `session_id` - The session ID to materialize
/// * `directory` - The project directory (used to derive the project key)
///
/// # Example
///
/// ```ignore
/// use claude_code_agent_sdk::session_store::{materialize_resume_session, InMemorySessionStore};
///
/// let store = Arc::new(InMemorySessionStore::new());
/// let materialized = materialize_resume_session(&*store, "session123", "/path/to/project").await?;
/// // Use materialized.session_id with Claude Code CLI
/// drop(materialized); // Cleanup happens automatically
/// ```
pub async fn materialize_resume_session(
    store: &dyn SessionStore,
    session_id: &str,
    directory: &Path,
) -> Result<MaterializedResume> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);

    // Load all entries for this session
    let entries = store.load(&key).await?;

    if entries.is_empty() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found or empty: {}",
            session_id
        )));
    }

    // Create a temporary config directory
    let temp_dir = tempfile::tempdir()
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to create temp dir: {}", e)))?;

    let config_dir = temp_dir.path().to_path_buf();

    // Create sessions directory
    let sessions_path = sessions_dir(&config_dir);
    fs::create_dir_all(&sessions_path)
        .map_err(|e| ClaudeError::Io(e))?;

    // Write session entries to file (JSONL format)
    let session_file_path = session_path(&config_dir, session_id);
    let file = fs::File::create(&session_file_path)
        .map_err(|e| ClaudeError::Io(e))?;
    let mut writer = std::io::BufWriter::new(file);

    for entry in &entries {
        let line = serde_json::to_string(entry)
            .map_err(|e| ClaudeError::InvalidConfig(e.to_string()))?;
        writeln!(writer, "{}", line)
            .map_err(|e| ClaudeError::Io(e))?;
    }

    writer.flush()
        .map_err(|e| ClaudeError::Io(e))?;

    // Copy auth files if they exist in the original config dir
    let original_config = directory
        .canonicalize()
        .ok()
        .and_then(|p| {
            // Walk up to find .claude directory
            let mut current = p.as_path();
            while let Some(parent) = current.parent() {
                let claude_dir = parent.join(".claude");
                if claude_dir.exists() {
                    return Some(claude_dir);
                }
                current = parent;
            }
            None
        });

    if let Some(original) = original_config {
        // Copy credentials if they exist
        let credentials_path = original.join("credentials.json");
        if credentials_path.exists() {
            let dest = config_dir.join("credentials.json");
            fs::copy(&credentials_path, &dest)
                .map_err(|e| ClaudeError::Io(e))?;
        }

        // Copy .claude.json if it exists
        let claude_json_path = original.join(".claude.json");
        if claude_json_path.exists() {
            let dest = config_dir.join(".claude.json");
            fs::copy(&claude_json_path, &dest)
                .map_err(|e| ClaudeError::Io(e))?;
        }
    }

    // Create cleanup closure
    Ok(MaterializedResume {
        config_dir,
        resume_session_id: session_id.to_string(),
        temp_dir,
    })
}

/// Get the project key for a directory
///
/// This is a convenience wrapper around the disk module's function.
pub fn get_project_key(directory: &Path) -> Result<String> {
    project_key_for_directory(directory)
}

/// Check if a session exists in the SessionStore
pub async fn session_exists(
    store: &dyn SessionStore,
    session_id: &str,
    directory: &Path,
) -> Result<bool> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);
    let entries = store.load(&key).await?;
    Ok(!entries.is_empty())
}

/// List all sessions for a project from a SessionStore
pub async fn list_project_sessions(
    store: &dyn SessionStore,
    directory: &Path,
) -> Result<Vec<String>> {
    let project_key = project_key_for_directory(directory)?;
    let entries = store.list_sessions(&project_key, None, None).await?;
    Ok(entries.into_iter().map(|e| e.session_id).collect())
}

/// Get the total size of a session in bytes
pub async fn get_session_size(
    store: &dyn SessionStore,
    session_id: &str,
    directory: &Path,
) -> Result<u64> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);
    let entries = store.load(&key).await?;

    let mut total_size = 0u64;
    for entry in entries {
        let entry_size = serde_json::to_string(&entry)
            .map(|s| s.len())
            .unwrap_or(0);
        total_size += entry_size as u64;
    }

    Ok(total_size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::InMemorySessionStore;
    use crate::types::session_store::{SessionKey, SessionStoreEntry};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_materialize_resume_session() {
        let store = Arc::new(InMemorySessionStore::new());
        let temp_dir = tempfile::tempdir().unwrap();

        // Use project key derived from temp_dir to ensure consistency
        let project_key = project_key_for_directory(temp_dir.path()).unwrap();

        // Add some test entries
        let key = SessionKey::new(&project_key, "session1");
        for i in 0..3 {
            let entry = SessionStoreEntry::new("user", format!("uuid{}", i), "2024-01-01T00:00:00Z");
            store.append(&key, entry).await.unwrap();
        }

        // Materialize
        let materialized = materialize_resume_session(&*store, "session1", temp_dir.path())
            .await
            .unwrap();

        // Verify
        assert_eq!(materialized.resume_session_id, "session1");
        assert!(materialized.session_path().exists());
    }

    #[tokio::test]
    async fn test_session_exists() {
        let store = Arc::new(InMemorySessionStore::new());
        let temp_dir = tempfile::tempdir().unwrap();

        // Use project key derived from temp_dir to ensure consistency
        let project_key = project_key_for_directory(temp_dir.path()).unwrap();

        let key = SessionKey::new(&project_key, "session1");
        let entry = SessionStoreEntry::new("user", "uuid1", "2024-01-01T00:00:00Z");
        store.append(&key, entry).await.unwrap();

        assert!(session_exists(&*store, "session1", temp_dir.path()).await.unwrap());
        assert!(!session_exists(&*store, "nonexistent", temp_dir.path()).await.unwrap());
    }
}
