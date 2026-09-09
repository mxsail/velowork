//! `ConnectionStore` — owns live SSH connection state.
//!
//! Previously `active_ssh_sessions` was a `HashSet<String>` field on
//! `Workspace`, mixed in with the project-layout and SSH-tree data and mutated
//! through blind `cx.notify()` methods. It now lives here as its own domain with
//! a typed `ConnectionEvent` so connection indicators (sidebar, terminal tabs)
//! subscribe precisely instead of re-rendering on any Workspace change.

use gpui::*;
use std::collections::{HashMap, HashSet};

/// Snapshot of a live connection.
#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub session_id: String,
    pub connected_at: i64,
}

/// Typed events for connection-state changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionEvent {
    /// A session connected (carries the session id).
    Connected(String),
    /// A session disconnected (carries the session id).
    Disconnected(String),
}

impl EventEmitter<ConnectionEvent> for ConnectionStore {}

/// Single-source owner of live SSH connection state.
pub struct ConnectionStore {
    active_ssh_sessions: HashSet<String>,
    connections: HashMap<String, ConnectionInfo>,
    recent_session_ids: Vec<String>,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl ConnectionStore {
    pub fn new() -> Self {
        Self {
            active_ssh_sessions: HashSet::new(),
            connections: HashMap::new(),
            recent_session_ids: Vec::new(),
        }
    }

    /// Whether a session currently has an active connection.
    pub fn is_connected(&self, session_id: &str) -> bool {
        self.active_ssh_sessions.contains(session_id)
    }

    /// All currently active SSH session ids.
    pub fn active_session_ids(&self) -> Vec<&str> {
        self.active_ssh_sessions.iter().map(|s| s.as_str()).collect()
    }

    /// Return the list of recently connected session IDs in MRU order.
    pub fn recent_session_ids(&self) -> &[String] {
        &self.recent_session_ids
    }

    /// Whether there are any active SSH sessions.
    pub fn has_active(&self) -> bool {
        !self.active_ssh_sessions.is_empty()
    }

    /// Number of currently active SSH sessions.
    pub fn active_count(&self) -> usize {
        self.active_ssh_sessions.len()
    }

    /// Record a connection. Only writer of `active_ssh_sessions`.
    pub fn mark_connected(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.active_ssh_sessions.insert(session_id.to_string());
        self.connections.insert(
            session_id.to_string(),
            ConnectionInfo {
                session_id: session_id.to_string(),
                connected_at: now_millis(),
            },
        );
        self.recent_session_ids.retain(|id| id != session_id);
        self.recent_session_ids.insert(0, session_id.to_string());
        if self.recent_session_ids.len() > 30 {
            self.recent_session_ids.truncate(30);
        }
        cx.emit(ConnectionEvent::Connected(session_id.to_string()));
        cx.notify();
    }

    /// Record a disconnection. Only writer of `active_ssh_sessions`.
    pub fn mark_disconnected(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.active_ssh_sessions.remove(session_id);
        self.connections.remove(session_id);
        cx.emit(ConnectionEvent::Disconnected(session_id.to_string()));
        cx.notify();
    }
}

/// Global handle for crate-level access.
#[derive(Clone)]
pub struct GlobalConnectionStore(pub Entity<ConnectionStore>);

impl Global for GlobalConnectionStore {}
