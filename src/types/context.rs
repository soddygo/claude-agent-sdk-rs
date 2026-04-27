//! Context Usage types for Claude Agent SDK
//!
//! This module provides types for the Context Usage API which returns
//! information about context window utilization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Category of context usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsageCategory {
    /// Category name
    pub name: String,
    /// Token count
    pub tokens: u32,
    /// Color for display
    pub color: String,
    /// Whether this is a deferred category
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_deferred: Option<bool>,
}

/// API usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiUsage {
    /// Total tokens used
    pub total_tokens: u32,
    /// Prompt tokens
    pub prompt_tokens: u32,
    /// Completion tokens
    pub completion_tokens: u32,
}

/// Context Usage Response
///
/// Returned by `ClaudeClient::get_context_usage()` to provide a breakdown
/// of context window utilization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUsageResponse {
    /// Categories of context usage
    pub categories: Vec<ContextUsageCategory>,
    /// Total tokens used
    pub total_tokens: u32,
    /// Maximum tokens allowed
    pub max_tokens: u32,
    /// Raw maximum tokens (before optimization)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_max_tokens: Option<u32>,
    /// Percentage of context used
    pub percentage: f32,
    /// Model being used
    pub model: String,
    /// Whether auto-compact is enabled
    pub is_auto_compact_enabled: bool,
    /// Memory files in context
    #[serde(default)]
    pub memory_files: Vec<String>,
    /// MCP tools in context
    #[serde(default)]
    pub mcp_tools: Vec<String>,
    /// Agents in context
    #[serde(default)]
    pub agents: Vec<String>,
    /// Grid rows
    pub grid_rows: u32,
    /// Auto-compact threshold
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_compact_threshold: Option<String>,
    /// Deferred builtin tools
    #[serde(default)]
    pub deferred_builtin_tools: Vec<String>,
    /// System tools
    #[serde(default)]
    pub system_tools: Vec<String>,
    /// System prompt sections
    #[serde(default)]
    pub system_prompt_sections: Vec<String>,
    /// Slash commands
    #[serde(default)]
    pub slash_commands: Vec<String>,
    /// Skills enabled
    #[serde(default)]
    pub skills: Vec<String>,
    /// Message breakdown by type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_breakdown: Option<HashMap<String, u32>>,
    /// API usage information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_usage: Option<ApiUsage>,
}
