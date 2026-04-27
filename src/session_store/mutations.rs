//! Session mutation operations
//!
//! This module provides functions for modifying sessions on disk:
//! rename, tag, delete, and fork.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use crate::errors::ClaudeError;
use crate::types::session_store::ForkSessionResult;
use crate::Result;
use crate::session_store::disk::session_path;

/// Rename a session (update its custom_title)
pub fn rename_session(
    session_id: &str,
    title: &str,
    directory: &Path,
) -> Result<()> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    // Read all lines
    let file = fs::File::open(&path)
        .map_err(|e| ClaudeError::Io(e))?;
    let reader = BufReader::new(file);

    let mut lines: Vec<String> = reader
        .lines()
        .map(|l| l.map_err(|e| ClaudeError::Io(e)))
        .collect::<Result<Vec<_>>>()?;

    // Update the first line (session metadata) with new title
    if !lines.is_empty() {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&lines[0]) {
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "custom_title".to_string(),
                    serde_json::json!(title),
                );
                // Also update summary to reflect the new title
                if let Some(summary) = obj.get_mut("summary") {
                    *summary = serde_json::json!(title);
                }
                lines[0] = serde_json::to_string(&json)
                    .map_err(|e| ClaudeError::InvalidConfig(e.to_string()))?;
            }
        }
    }

    // Write back
    let file = fs::File::create(&path)
        .map_err(|e| ClaudeError::Io(e))?;
    let mut writer = std::io::BufWriter::new(file);
    for line in &lines {
        writeln!(writer, "{}", line)
            .map_err(|e| ClaudeError::Io(e))?;
    }

    Ok(())
}

/// Add or update a tag on a session
pub fn tag_session(
    session_id: &str,
    tag: &str,
    directory: &Path,
) -> Result<()> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    // Read all lines
    let file = fs::File::open(&path)
        .map_err(|e| ClaudeError::Io(e))?;
    let reader = BufReader::new(file);

    let mut lines: Vec<String> = reader
        .lines()
        .map(|l| l.map_err(|e| ClaudeError::Io(e)))
        .collect::<Result<Vec<_>>>()?;

    // Update the first line (session metadata) with new tag
    if !lines.is_empty() {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&lines[0]) {
            if let Some(obj) = json.as_object_mut() {
                obj.insert(
                    "tag".to_string(),
                    serde_json::json!(tag),
                );
                lines[0] = serde_json::to_string(&json)
                    .map_err(|e| ClaudeError::InvalidConfig(e.to_string()))?;
            }
        }
    }

    // Write back
    let file = fs::File::create(&path)
        .map_err(|e| ClaudeError::Io(e))?;
    let mut writer = std::io::BufWriter::new(file);
    for line in &lines {
        writeln!(writer, "{}", line)
            .map_err(|e| ClaudeError::Io(e))?;
    }

    Ok(())
}

/// Delete a session
pub fn delete_session(session_id: &str, directory: &Path) -> Result<()> {
    let path = session_path(directory, session_id);

    if !path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    fs::remove_file(&path)
        .map_err(|e| ClaudeError::Io(e))?;

    // Also try to delete associated .channel.json file if exists
    let channel_path = path.with_extension("channel.json");
    if channel_path.exists() {
        let _ = fs::remove_file(channel_path);
    }

    Ok(())
}

/// Fork a session, creating a new session with the same transcript
///
/// If `up_to_message_id` is provided, only messages up to and including
/// that message ID will be included in the fork.
pub fn fork_session(
    session_id: &str,
    directory: &Path,
    up_to_message_id: Option<&str>,
    title: Option<&str>,
) -> Result<ForkSessionResult> {
    let source_path = session_path(directory, session_id);

    if !source_path.exists() {
        return Err(ClaudeError::InvalidConfig(format!(
            "Session not found: {}",
            session_id
        )));
    }

    // Generate new session ID
    let new_session_id = uuid::Uuid::new_v4().to_string();

    // Read source session
    let content = fs::read_to_string(&source_path)
        .map_err(|e| ClaudeError::Io(e))?;

    // Parse and filter messages if up_to_message_id is specified
    let new_lines: Vec<String> = if let Some(stop_uuid) = up_to_message_id {
        content
            .lines()
            .take_while(|line| {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
                    // Check if this message has the stop UUID
                    if let Some(uuid_val) = json.get("uuid").and_then(|v| v.as_str()) {
                        return uuid_val != stop_uuid;
                    }
                }
                true
            })
            .map(|s| s.to_string())
            .collect()
    } else {
        content.lines().map(|s| s.to_string()).collect()
    };

    // Update metadata in new session
    if !new_lines.is_empty() {
        // Parse and update the first line (session metadata)
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&new_lines[0]) {
            if let Some(obj) = json.as_object_mut() {
                // Update session_id
                obj.insert("session_id".to_string(), serde_json::json!(new_session_id));

                // Update created_at
                obj.insert(
                    "created_at".to_string(),
                    serde_json::json!(chrono::Utc::now().to_rfc3339()),
                );

                // Update title if provided
                if let Some(t) = title {
                    obj.insert("custom_title".to_string(), serde_json::json!(t));
                    obj.insert("summary".to_string(), serde_json::json!(t));
                }

                // Update last_modified
                obj.insert(
                    "last_modified".to_string(),
                    serde_json::json!(chrono::Utc::now().to_rfc3339()),
                );
            }
            // Store back the modified first line
            let new_first_line = serde_json::to_string(&json)
                .map_err(|e| ClaudeError::InvalidConfig(e.to_string()))?;
            let mut final_lines = vec![new_first_line];
            final_lines.extend(new_lines[1..].to_vec());
        }
    }

    // Write new session file
    let new_path = session_path(directory, &new_session_id);
    let file = fs::File::create(&new_path)
        .map_err(|e| ClaudeError::Io(e))?;
    let mut writer = std::io::BufWriter::new(file);

    for line in &new_lines {
        writeln!(writer, "{}", line)
            .map_err(|e| ClaudeError::Io(e))?;
    }

    Ok(ForkSessionResult { session_id: new_session_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::disk::sessions_dir;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_rename_session() {
        let temp_dir = TempDir::new().unwrap();
        let session_id = "test_session";

        // Create the sessions directory
        let sessions_dir = sessions_dir(temp_dir.path());
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Create a test session file
        let path = session_path(temp_dir.path(), session_id);
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"session","session_id":"{}","summary":"original","custom_title":null}}"#,
            session_id
        )
        .unwrap();

        rename_session(session_id, "New Title", temp_dir.path()).unwrap();

        // Verify
        let content = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content.lines().next().unwrap()).unwrap();
        assert_eq!(json["custom_title"], "New Title");
        assert_eq!(json["summary"], "New Title");
    }

    #[test]
    fn test_delete_session() {
        let temp_dir = TempDir::new().unwrap();
        let session_id = "test_session";

        // Create the sessions directory
        let sessions_dir = sessions_dir(temp_dir.path());
        std::fs::create_dir_all(&sessions_dir).unwrap();

        // Create a test session file
        let path = session_path(temp_dir.path(), session_id);
        std::fs::write(&path, "test content").unwrap();

        delete_session(session_id, temp_dir.path()).unwrap();

        assert!(!path.exists());
    }
}
