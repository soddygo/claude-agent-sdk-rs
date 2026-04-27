//! In-memory session store implementation

use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;

use crate::Result;
use crate::types::session_store::{
    SessionKey, SessionListEntry, SessionListSubkeysKey, SessionStore, SessionStoreEntry,
};

/// In-memory session store implementation using DashMap
///
/// This implementation uses DashMap for concurrent access without locking,
/// following best practices from the user's CLAUDE.md (using entry() API
/// to avoid deadlock issues with dashmap in concurrent scenarios).
///
/// # Example
///
/// ```ignore
/// use claude_code_agent_sdk::session_store::InMemorySessionStore;
///
/// let store = InMemorySessionStore::new();
/// ```
#[derive(Debug, Clone)]
pub struct InMemorySessionStore {
    /// Entries stored by session key
    entries: DashMap<SessionKey, Vec<SessionStoreEntry>>,
    /// Session list by project_key -> list of sessions
    sessions: DashMap<String, Vec<SessionListEntry>>,
    /// Subkeys by project_key/session_id
    subkeys: DashMap<SessionListSubkeysKey, Vec<String>>,
}

impl InMemorySessionStore {
    /// Create a new in-memory session store
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            sessions: DashMap::new(),
            subkeys: DashMap::new(),
        }
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn append(
        &self,
        session_key: &SessionKey,
        entry: SessionStoreEntry,
    ) -> Result<()> {
        // Use entry() API to avoid deadlock per user guidelines
        let mut entries = self.entries.entry(session_key.clone()).or_default();
        entries.push(entry);
        Ok(())
    }

    async fn load(&self, session_key: &SessionKey) -> Result<Vec<SessionStoreEntry>> {
        // Use get() instead of entry() for read-only access
        if let Some(entries) = self.entries.get(session_key) {
            Ok(entries.clone())
        } else {
            Ok(Vec::new())
        }
    }

    async fn list_sessions(
        &self,
        project_key: &str,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<SessionListEntry>> {
        let sessions = self.sessions.get(project_key);

        let sessions_vec = match sessions {
            Some(s) => s.clone(),
            None => return Ok(Vec::new()),
        };

        let offset = offset.unwrap_or(0);
        let limit = limit.unwrap_or(usize::MAX);

        let end = std::cmp::min(offset.saturating_add(limit), sessions_vec.len());

        if offset >= sessions_vec.len() {
            return Ok(Vec::new());
        }

        Ok(sessions_vec[offset..end].to_vec())
    }

    async fn delete(&self, session_key: &SessionKey) -> Result<()> {
        // Remove entries
        self.entries.remove(session_key);

        // Remove from sessions list
        if let Some(mut sessions) = self.sessions.get_mut(&session_key.project_key) {
            sessions.retain(|s| s.session_id != session_key.session_id);
        }

        // Remove subkeys
        let subkey = SessionListSubkeysKey {
            project_key: session_key.project_key.clone(),
            session_id: session_key.session_id.clone(),
        };
        self.subkeys.remove(&subkey);

        Ok(())
    }

    async fn list_subkeys(
        &self,
        key: &SessionListSubkeysKey,
    ) -> Result<Vec<String>> {
        if let Some(subkeys) = self.subkeys.get(key) {
            Ok(subkeys.clone())
        } else {
            Ok(Vec::new())
        }
    }
}

impl InMemorySessionStore {
    /// Add a session to the project's session list
    pub fn add_session_entry(&self, project_key: &str, entry: SessionListEntry) {
        // Use entry() API for write access
        let mut sessions = self.sessions.entry(project_key.to_string()).or_default();

        // Check if session already exists and update mtime
        if let Some(existing) = sessions.iter_mut().find(|s| s.session_id == entry.session_id) {
            existing.mtime = entry.mtime;
        } else {
            sessions.push(entry);
        }
    }

    /// Add a subkey (e.g., subagent ID) to a session
    pub fn add_subkey(&self, key: &SessionListSubkeysKey, subkey: String) {
        let mut subkeys = self.subkeys.entry(key.clone()).or_default();
        if !subkeys.contains(&subkey) {
            subkeys.push(subkey);
        }
    }

    /// Get a clone of the store wrapped in Arc for sharing across tasks
    pub fn arc(self: &Self) -> Arc<Self> {
        Arc::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::session_store::SessionStoreEntry;

    #[tokio::test]
    async fn test_append_and_load() {
        let store = InMemorySessionStore::new();
        let key = SessionKey::new("project1", "session1");
        let entry = SessionStoreEntry::new("user", "uuid1", "2024-01-01T00:00:00Z");

        store.append(&key, entry.clone()).await.unwrap();

        let entries = store.load(&key).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, "user");
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let store = InMemorySessionStore::new();

        store.add_session_entry("project1", SessionListEntry {
            session_id: "session1".to_string(),
            mtime: "2024-01-01T00:00:00Z".to_string(),
        });

        let sessions = store.list_sessions("project1", None, None).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session1");
    }

    #[tokio::test]
    async fn test_delete() {
        let store = InMemorySessionStore::new();
        let key = SessionKey::new("project1", "session1");
        let entry = SessionStoreEntry::new("user", "uuid1", "2024-01-01T00:00:00Z");

        store.append(&key, entry).await.unwrap();
        store.delete(&key).await.unwrap();

        let entries = store.load(&key).await.unwrap();
        assert!(entries.is_empty());
    }
}
