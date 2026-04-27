//! Disk-based session reading implementation
//!
//! This module provides functions for reading sessions from the Claude Code
//! CLI's on-disk session storage format.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use crate::errors::ClaudeError;
use crate::types::session_store::{SDKSessionInfo, SessionMessage};
use crate::Result;

/// Get the default Claude config directory
pub fn default_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("claude"))
}

/// Get the sessions directory path
pub fn sessions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("sessions")
}

/// Get the path to a specific session file
pub fn session_path(config_dir: &Path, session_id: &str) -> PathBuf {
    sessions_dir(config_dir).join(format!("{}.jsonl", session_id))
}

/// List all sessions for a project in a directory
///
/// Sessions are stored as JSONL files in the sessions directory.
/// Each line is a JSON object with session metadata.
pub fn list_sessions(
    directory: &Path,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SDKSessionInfo>> {
    let sessions_path = sessions_dir(directory);

    if !sessions_path.exists() {
        return Ok(Vec::new());
    }

    let mut sessions: Vec<SDKSessionInfo> = Vec::new();

    let entries = fs::read_dir(&sessions_path)
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to read sessions dir: {}", e)))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        if let Ok(info) = read_session_info_from_file(&path) {
            sessions.push(info);
        }
    }

    // Sort by last_modified descending (most recent first)
    sessions.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(usize::MAX);

    let end = std::cmp::min(offset.saturating_add(limit), sessions.len());

    if offset >= sessions.len() {
        return Ok(Vec::new());
    }

    Ok(sessions[offset..end].to_vec())
}

/// Read session info from a session file (just head/tail for metadata)
fn read_session_info_from_file(path: &Path) -> Result<SDKSessionInfo> {
    let content = fs::read_to_string(path)
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to read session file: {}", e)))?;

    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    let metadata = extract_session_metadata(&content)?;

    let file_size = fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(SDKSessionInfo {
        session_id,
        summary: metadata.get("summary").cloned().unwrap_or_default(),
        last_modified: metadata.get("last_modified").cloned().unwrap_or_default(),
        file_size,
        custom_title: metadata.get("custom_title").cloned(),
        first_prompt: metadata.get("first_prompt").cloned(),
        git_branch: metadata.get("git_branch").cloned(),
        cwd: metadata.get("cwd").cloned(),
        tag: metadata.get("tag").cloned(),
        created_at: metadata.get("created_at").cloned().unwrap_or_default(),
    })
}

/// Extract metadata from session file content
fn extract_session_metadata(content: &str) -> Result<std::collections::HashMap<String, String>> {
    let mut metadata = std::collections::HashMap::new();

    // First line usually contains session metadata
    if let Some(first_line) = content.lines().next() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(first_line) {
            // Session metadata is typically at the start
            if let Some(obj) = json.as_object() {
                // Look for metadata fields
                for (key, value) in obj {
                    if let Some(s) = value.as_str() {
                        if matches!(
                            key.as_str(),
                            "summary" | "last_modified" | "custom_title"
                                | "first_prompt" | "git_branch" | "cwd" | "tag" | "created_at"
                        ) {
                            metadata.insert(key.clone(), s.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(metadata)
}

/// Get detailed info about a specific session
pub fn get_session_info(session_id: &str, directory: &Path) -> Result<SDKSessionInfo> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    read_session_info_from_file(&path)
}

/// Get messages from a session with optional pagination
pub fn get_session_messages(
    session_id: &str,
    directory: &Path,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SessionMessage>> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to read session file: {}", e)))?;

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(usize::MAX);

    let messages: Vec<SessionMessage> = content
        .lines()
        .skip(offset)
        .take(limit)
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    Ok(messages)
}

/// List all subagent IDs for a session
pub fn list_subagents(session_id: &str, directory: &Path) -> Result<Vec<String>> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to read session file: {}", e)))?;

    let mut subagent_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in content.lines() {
        if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
            // Check if this is a subagent message
            if let Some(data_obj) = msg.get("data").and_then(|d| d.as_object()) {
                if let Some(agent_id) = data_obj.get("agent_id").and_then(|v| v.as_str()) {
                    subagent_ids.insert(agent_id.to_string());
                }
            }
        }
    }

    Ok(subagent_ids.into_iter().collect())
}

/// Get messages for a specific subagent within a session
pub fn get_subagent_messages(
    session_id: &str,
    agent_id: &str,
    directory: &Path,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SessionMessage>> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to read session file: {}", e)))?;

    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(usize::MAX);

    let messages: Vec<SessionMessage> = content
        .lines()
        .filter_map(|line| {
            let msg: serde_json::Result<serde_json::Value> = serde_json::from_str(line);
            match msg {
                Ok(json) => {
                    // Filter by agent_id
                    if let Some(data_obj) = json.get("data").and_then(|d| d.as_object()) {
                        if data_obj.get("agent_id").and_then(|v| v.as_str()) == Some(agent_id) {
                            return SessionMessage::deserialize(json).ok();
                        }
                    }
                    None
                }
                Err(_) => None,
            }
        })
        .skip(offset)
        .take(limit)
        .collect();

    Ok(messages)
}

/// Get the project key for a directory
///
/// The project key is derived from the absolute path of the directory.
pub fn project_key_for_directory(directory: &Path) -> Result<String> {
    let abs_path = directory
        .canonicalize()
        .map_err(|e| ClaudeError::InvalidConfig(format!("Failed to get absolute path: {}", e)))?;

    // Use a hash of the path to create a safe key
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    abs_path.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();

    Ok(format!("{:016x}", hash))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_project_key_deterministic() {
        let temp_dir = TempDir::new().unwrap();
        let dir1 = temp_dir.path();
        let dir2 = temp_dir.path();

        let key1 = project_key_for_directory(dir1).unwrap();
        let key2 = project_key_for_directory(dir2).unwrap();

        assert_eq!(key1, key2);
    }
}
