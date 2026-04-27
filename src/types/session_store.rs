//! Session Store types for Claude Agent SDK
//!
//! This module provides the SessionStore trait and related types for implementing
//! custom session storage backends,参考 Python SDK's SessionStore Protocol.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::Result;

/// Session key identifying a unique session
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionKey {
    /// Project identifier
    pub project_key: String,
    /// Session identifier
    pub session_id: String,
    /// Optional subpath for nested sessions (e.g., subagent transcripts)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
}

impl SessionKey {
    /// Create a new session key
    pub fn new(project_key: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            project_key: project_key.into(),
            session_id: session_id.into(),
            subpath: None,
        }
    }

    /// Create a session key with a subpath
    pub fn with_subpath(
        project_key: impl Into<String>,
        session_id: impl Into<String>,
        subpath: impl Into<String>,
    ) -> Self {
        Self {
            project_key: project_key.into(),
            session_id: session_id.into(),
            subpath: Some(subpath.into()),
        }
    }
}

/// A single entry in a session transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStoreEntry {
    /// Entry type (e.g., "user", "assistant", "system", "result")
    #[serde(rename = "type")]
    pub entry_type: String,
    /// Unique identifier for this entry
    pub uuid: String,
    /// ISO 8601 timestamp
    pub timestamp: String,
    /// Entry data (varies by type)
    #[serde(flatten)]
    pub data: HashMap<String, serde_json::Value>,
}

impl SessionStoreEntry {
    /// Create a new session store entry
    pub fn new(
        entry_type: impl Into<String>,
        uuid: impl Into<String>,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            entry_type: entry_type.into(),
            uuid: uuid.into(),
            timestamp: timestamp.into(),
            data: HashMap::new(),
        }
    }

    /// Add a data field to the entry
    pub fn with_data(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.data.insert(key.into(), value);
        self
    }
}

/// Entry for listing sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListEntry {
    /// Session identifier
    pub session_id: String,
    /// Last modified time (ISO 8601)
    pub mtime: String,
}

/// Key for listing session subkeys (e.g., subagent IDs)
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionListSubkeysKey {
    /// Project identifier
    pub project_key: String,
    /// Session identifier
    pub session_id: String,
}

/// Session information returned by list_sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SDKSessionInfo {
    /// Session identifier
    pub session_id: String,
    /// Session summary (usually first user message)
    pub summary: String,
    /// Last modified time (ISO 8601)
    pub last_modified: String,
    /// File size in bytes
    pub file_size: u64,
    /// Custom title if set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_title: Option<String>,
    /// First prompt in the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_prompt: Option<String>,
    /// Git branch when session was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    /// Working directory when session was created
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Session tag
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Creation timestamp (ISO 8601)
    pub created_at: String,
}

/// A message within a session transcript
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// Message type ("user" or "assistant")
    #[serde(rename = "type")]
    pub msg_type: String,
    /// Message UUID
    pub uuid: String,
    /// Session identifier
    pub session_id: String,
    /// Message content
    pub message: serde_json::Value,
    /// Parent tool use ID for assistant messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_use_id: Option<String>,
}

/// Result of forking a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForkSessionResult {
    /// The new session ID after forking
    pub session_id: String,
}

/// SessionStore trait for implementing custom session storage backends
///
/// This trait defines the interface for storing and retrieving session data,
/// allowing users to implement their own storage solutions (e.g., database,
/// cloud storage, distributed cache).
///
/// # Example
///
/// ```ignore
/// use claude_code_agent_sdk::types::session_store::{SessionStore, SessionKey, SessionStoreEntry};
/// use std::sync::Arc;
///
/// struct MyStore {
///     // custom storage implementation
/// }
///
/// #[async_trait]
/// impl SessionStore for MyStore {
///     async fn append(&self, session_key: &SessionKey, entry: SessionStoreEntry) -> Result<(), ClaudeError> {
///         // custom implementation
///         Ok(())
///     }
///     // ... implement other methods
/// }
/// ```
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Append a new entry to a session transcript
    async fn append(
        &self,
        session_key: &SessionKey,
        entry: SessionStoreEntry,
    ) -> Result<()>;

    /// Load all entries for a session
    async fn load(&self, session_key: &SessionKey) -> Result<Vec<SessionStoreEntry>>;

    /// List all sessions for a project
    async fn list_sessions(
        &self,
        project_key: &str,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<SessionListEntry>>;

    /// Delete a session and all its entries
    async fn delete(&self, session_key: &SessionKey) -> Result<()>;

    /// List subkeys for a session (e.g., subagent IDs)
    async fn list_subkeys(
        &self,
        key: &SessionListSubkeysKey,
    ) -> Result<Vec<String>>;
}
