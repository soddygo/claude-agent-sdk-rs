//! SessionStore-backed API functions
//!
//! This module provides async versions of the session functions
//! that work with a SessionStore implementation.

use std::path::Path;
use std::sync::Arc;

use crate::errors::ClaudeError;
use crate::types::session_store::{
    ForkSessionResult, SDKSessionInfo, SessionKey, SessionListSubkeysKey,
    SessionMessage, SessionStore,
};
use crate::Result;
use crate::session_store::disk::project_key_for_directory;

/// List sessions from a SessionStore
pub async fn list_sessions_from_store(
    store: Arc<dyn SessionStore>,
    directory: &Path,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SDKSessionInfo>> {
    let project_key = project_key_for_directory(directory)?;

    // Load all entries and group by session
    let list_entries = store
        .list_sessions(&project_key, None, None)
        .await?;

    // For each session, get the first entry to extract metadata
    let mut sessions: Vec<SDKSessionInfo> = Vec::new();

    for entry in list_entries {
        let key = SessionKey::new(&project_key, &entry.session_id);
        let entries = store.load(&key).await?;

        if let Some(first) = entries.first() {
            let info = SDKSessionInfo {
                session_id: entry.session_id,
                summary: first
                    .data
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                last_modified: entry.mtime,
                file_size: first
                    .data
                    .get("file_size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                custom_title: first.data.get("custom_title").and_then(|v| v.as_str()).map(String::from),
                first_prompt: first.data.get("first_prompt").and_then(|v| v.as_str()).map(String::from),
                git_branch: first.data.get("git_branch").and_then(|v| v.as_str()).map(String::from),
                cwd: first.data.get("cwd").and_then(|v| v.as_str()).map(String::from),
                tag: first.data.get("tag").and_then(|v| v.as_str()).map(String::from),
                created_at: first.timestamp.clone(),
            };
            sessions.push(info);
        }
    }

    // Apply pagination
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(usize::MAX);
    let end = std::cmp::min(offset.saturating_add(limit), sessions.len());

    if offset >= sessions.len() {
        return Ok(Vec::new());
    }

    // Sort by last_modified descending
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    Ok(sessions[offset..end].to_vec())
}

/// Get session info from a SessionStore
pub async fn get_session_info_from_store(
    store: Arc<dyn SessionStore>,
    session_id: &str,
    directory: &Path,
) -> Result<SDKSessionInfo> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);
    let entries = store.load(&key).await?;

    if entries.is_empty() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    let first = &entries[0];

    Ok(SDKSessionInfo {
        session_id: session_id.to_string(),
        summary: first
            .data
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        last_modified: first.timestamp.clone(),
        file_size: first
            .data
            .get("file_size")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        custom_title: first.data.get("custom_title").and_then(|v| v.as_str()).map(String::from),
        first_prompt: first.data.get("first_prompt").and_then(|v| v.as_str()).map(String::from),
        git_branch: first.data.get("git_branch").and_then(|v| v.as_str()).map(String::from),
        cwd: first.data.get("cwd").and_then(|v| v.as_str()).map(String::from),
        tag: first.data.get("tag").and_then(|v| v.as_str()).map(String::from),
        created_at: first.timestamp.clone(),
    })
}

/// Get session messages from a SessionStore
pub async fn get_session_messages_from_store(
    store: Arc<dyn SessionStore>,
    session_id: &str,
    directory: &Path,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SessionMessage>> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);
    let entries = store.load(&key).await?;

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(usize::MAX);

    let messages: Vec<SessionMessage> = entries
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|e| SessionMessage {
            msg_type: e.entry_type,
            uuid: e.uuid,
            session_id: session_id.to_string(),
            message: serde_json::to_value(&e.data).unwrap_or_default(),
            parent_tool_use_id: e.data.get("parent_tool_use_id").and_then(|v| v.as_str()).map(String::from),
        })
        .collect();

    Ok(messages)
}

/// List subagents from a SessionStore
pub async fn list_subagents_from_store(
    store: Arc<dyn SessionStore>,
    session_id: &str,
    directory: &Path,
) -> Result<Vec<String>> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionListSubkeysKey {
        project_key: project_key.clone(),
        session_id: session_id.to_string(),
    };

    store.list_subkeys(&key).await
}

/// Get subagent messages from a SessionStore
pub async fn get_subagent_messages_from_store(
    store: Arc<dyn SessionStore>,
    session_id: &str,
    agent_id: &str,
    directory: &Path,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SessionMessage>> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);
    let entries = store.load(&key).await?;

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(usize::MAX);

    let messages: Vec<SessionMessage> = entries
        .into_iter()
        .filter(|e| {
            e.data
                .get("agent_id")
                .and_then(|v| v.as_str())
                .map(|id| id == agent_id)
                .unwrap_or(false)
        })
        .skip(offset)
        .take(limit)
        .map(|e| SessionMessage {
            msg_type: e.entry_type,
            uuid: e.uuid,
            session_id: session_id.to_string(),
            message: serde_json::to_value(&e.data).unwrap_or_default(),
            parent_tool_use_id: e.data.get("parent_tool_use_id").and_then(|v| v.as_str()).map(String::from),
        })
        .collect();

    Ok(messages)
}

/// Rename a session via SessionStore
///
/// Note: This only updates the in-memory store. For persistent changes,
/// you would need to implement persistence in your SessionStore implementation.
pub async fn rename_session_via_store(
    _store: Arc<dyn SessionStore>,
    _session_id: &str,
    _title: &str,
    _directory: &Path,
) -> Result<()> {
    // For in-memory stores, this is a no-op since there's no persistence
    // Users should implement this in their persistent SessionStore implementations
    Err(ClaudeError::InvalidConfig(
        "rename_session_via_store requires a persistent SessionStore implementation".to_string(),
    ))
}

/// Tag a session via SessionStore
pub async fn tag_session_via_store(
    _store: Arc<dyn SessionStore>,
    _session_id: &str,
    _tag: &str,
    _directory: &Path,
) -> Result<()> {
    Err(ClaudeError::InvalidConfig(
        "tag_session_via_store requires a persistent SessionStore implementation".to_string(),
    ))
}

/// Delete a session via SessionStore
pub async fn delete_session_via_store(
    store: Arc<dyn SessionStore>,
    session_id: &str,
    directory: &Path,
) -> Result<()> {
    let project_key = project_key_for_directory(directory)?;
    let key = SessionKey::new(&project_key, session_id);
    store.delete(&key).await
}

/// Fork a session via SessionStore
pub async fn fork_session_via_store(
    _store: Arc<dyn SessionStore>,
    _session_id: &str,
    _directory: &Path,
    _up_to_message_id: Option<&str>,
    _title: Option<&str>,
) -> Result<ForkSessionResult> {
    Err(ClaudeError::InvalidConfig(
        "fork_session_via_store requires a persistent SessionStore implementation".to_string(),
    ))
}
