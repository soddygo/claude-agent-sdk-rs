//! Transcript mirror batcher for efficient session store updates
//!
//! This module provides batching functionality to reduce the number of
//! writes to a SessionStore by accumulating entries and flushing them
//! in bulk when thresholds are reached.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tokio::time::timeout;

use crate::errors::ClaudeError;
use crate::types::session_store::{SessionKey, SessionStore, SessionStoreEntry};
use crate::Result;

/// Default threshold for number of entries before flush
const DEFAULT_FLUSH_THRESHOLD: usize = 500;

/// Default threshold for size in bytes before flush
const DEFAULT_SIZE_THRESHOLD: usize = 1024 * 1024; // 1MB

/// Default timeout for batcher operations
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// TranscriptMirrorBatcher accumulates entries and flushes them in batches
///
/// This is similar to Python SDK's TranscriptMirrorBatcher which batches
/// transcript_mirror frames for efficient SessionStore.append calls.
///
/// The batcher uses thresholds to trigger automatic flushes:
/// - Entry count threshold (default 500)
/// - Size threshold (default 1MB)
///
/// # Example
///
/// ```ignore
/// use claude_code_agent_sdk::session_store::{TranscriptMirrorBatcher, InMemorySessionStore};
/// use claude_code_agent_sdk::types::session_store::{SessionKey, SessionStoreEntry};
///
/// let store = Arc::new(InMemorySessionStore::new());
/// let batcher = TranscriptMirrorBatcher::new(store);
///
/// // Append entries (they get batched)
/// batcher.append(key, entry).await?;
/// batcher.flush().await?;
/// ```
pub struct TranscriptMirrorBatcher {
    /// The underlying session store
    store: Arc<dyn SessionStore>,
    /// Pending entries grouped by session key
    pending: DashMap<SessionKey, Vec<SessionStoreEntry>>,
    /// Pending size in bytes per session
    pending_size: DashMap<SessionKey, usize>,
    /// Threshold for number of entries before flush
    flush_threshold: usize,
    /// Threshold for size in bytes before flush
    size_threshold: usize,
    /// Sender for shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TranscriptMirrorBatcher {
    /// Create a new batcher with the given store and default thresholds
    pub fn new(store: Arc<dyn SessionStore>) -> Self {
        Self {
            store,
            pending: DashMap::new(),
            pending_size: DashMap::new(),
            flush_threshold: DEFAULT_FLUSH_THRESHOLD,
            size_threshold: DEFAULT_SIZE_THRESHOLD,
            shutdown_tx: None,
        }
    }

    /// Create a new batcher with custom thresholds
    pub fn with_thresholds(
        store: Arc<dyn SessionStore>,
        flush_threshold: usize,
        size_threshold: usize,
    ) -> Self {
        Self {
            store,
            pending: DashMap::new(),
            pending_size: DashMap::new(),
            flush_threshold,
            size_threshold,
            shutdown_tx: None,
        }
    }

    /// Append an entry to the batch
    ///
    /// If thresholds are reached, automatically triggers a flush.
    pub async fn append(
        &self,
        session_key: SessionKey,
        entry: SessionStoreEntry,
    ) -> Result<()> {
        // Calculate entry size
        let entry_size = serde_json::to_string(&entry)
            .map(|s| s.len())
            .unwrap_or(0);

        // Use entry() API to avoid deadlock
        let mut entries = self.pending.entry(session_key.clone()).or_default();
        entries.push(entry);

        // Update size tracking
        let mut size = self.pending_size.entry(session_key.clone()).or_default();
        *size += entry_size;

        // Check if we should flush
        if entries.len() >= self.flush_threshold || *size >= self.size_threshold {
            drop(entries);
            drop(size);
            self.flush_for_key(&session_key).await?;
        }

        Ok(())
    }

    /// Flush all pending entries for a specific session key
    pub async fn flush_for_key(&self, session_key: &SessionKey) -> Result<()> {
        // Take all pending entries for this key using entry() API
        let entries: Vec<SessionStoreEntry> = self
            .pending
            .entry(session_key.clone())
            .or_default()
            .drain(..)
            .collect();

        // Reset size
        self.pending_size
            .entry(session_key.clone())
            .and_modify(|s| *s = 0);

        // Write to store if we have entries
        if !entries.is_empty() {
            let store = self.store.clone();
            let key = session_key.clone();

            timeout(DEFAULT_TIMEOUT, async move {
                for entry in entries {
                    store.append(&key, entry).await?;
                }
                Ok::<(), ClaudeError>(())
            })
            .await
            .map_err(|_| ClaudeError::Timeout("flush timed out".to_string()))??;
        }

        Ok(())
    }

    /// Flush all pending entries across all sessions
    pub async fn flush(&self) -> Result<()> {
        // Collect all keys by iterating
        let keys: Vec<SessionKey> = self.pending.iter().map(|r| r.key().clone()).collect();

        for key in keys {
            self.flush_for_key(&key).await?;
        }

        Ok(())
    }

    /// Get the number of pending entries for a session
    pub fn pending_count(&self, session_key: &SessionKey) -> usize {
        self.pending
            .get(session_key)
            .map(|e| e.len())
            .unwrap_or(0)
    }

    /// Get total pending entry count across all sessions
    pub fn total_pending_count(&self) -> usize {
        self.pending.iter().map(|e| e.len()).sum()
    }

    /// Get the number of sessions with pending entries
    pub fn pending_session_count(&self) -> usize {
        self.pending.len()
    }

    /// Start the background flush task
    ///
    /// This spawns a task that periodically flushes pending entries.
    /// The task runs until the returned handle is dropped or `shutdown` is called.
    pub fn start_background_flush(self: &Arc<Self>, interval: Duration) -> BatcherHandle {
        let (tx, mut rx) = mpsc::channel::<()>(1);
        let batcher = Arc::clone(self);

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        if let Err(e) = batcher.flush().await {
                            tracing::error!("background flush error: {}", e);
                        }
                    }
                    _ = rx.recv() => {
                        // Shutdown signal received
                        tracing::debug!("batcher shutdown signal received");
                        break;
                    }
                }
            }

            // Final flush on shutdown
            if let Err(e) = batcher.flush().await {
                tracing::error!("final flush error on shutdown: {}", e);
            }
        });

        BatcherHandle { tx }
    }

    /// Shutdown the batcher and flush all pending entries
    pub async fn shutdown(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        self.flush().await
    }
}

/// Handle to control the background flush task
#[derive(Debug)]
pub struct BatcherHandle {
    tx: mpsc::Sender<()>,
}

impl BatcherHandle {
    /// Signal the background task to shutdown
    pub async fn shutdown(self) -> Result<()> {
        let _ = self.tx.send(()).await;
        Ok(())
    }
}

impl Drop for BatcherHandle {
    fn drop(&mut self) {
        // Signal shutdown on drop - best effort, don't wait
        let _ = self.tx.try_send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::InMemorySessionStore;
    use crate::types::session_store::{SessionKey, SessionStoreEntry};

    #[tokio::test]
    async fn test_batcher_append_and_flush() {
        let store = Arc::new(InMemorySessionStore::new());
        let batcher = TranscriptMirrorBatcher::with_thresholds(store.clone(), 100, 1024 * 1024);

        let key = SessionKey::new("project", "session");
        let entry = SessionStoreEntry::new("user", "uuid1", "2024-01-01T00:00:00Z");

        batcher.append(key.clone(), entry).await.unwrap();
        assert_eq!(batcher.total_pending_count(), 1);

        batcher.flush().await.unwrap();
        assert_eq!(batcher.total_pending_count(), 0);

        // Verify entry was written to store
        let entries = store.load(&key).await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_batcher_auto_flush() {
        let store = Arc::new(InMemorySessionStore::new());
        // Set low thresholds to trigger auto-flush
        let batcher = TranscriptMirrorBatcher::with_thresholds(store.clone(), 2, 1024);

        let key = SessionKey::new("project", "session");

        // Add 2 entries (should trigger auto-flush at threshold)
        for i in 0..2 {
            let entry = SessionStoreEntry::new("user", format!("uuid{}", i), "2024-01-01T00:00:00Z");
            batcher.append(key.clone(), entry).await.unwrap();
        }

        // Should have auto-flushed
        assert_eq!(batcher.total_pending_count(), 0);

        // Verify entries in store
        let entries = store.load(&key).await.unwrap();
        assert_eq!(entries.len(), 2);
    }
}
