//! Transient project lifecycle state.
//!
//! Tracks which projects are currently being created, closed, or removed.
//!
//! None of this is persisted — everything resets on restart.

use std::collections::HashSet;

/// Tracks transient "is this project being created/closed/removed" state.
#[derive(Debug, Default)]
pub struct ProjectLifecycleTracker {
    /// Project IDs whose worktree is still being created on disk.
    creating: HashSet<String>,
    /// Project IDs currently being closed (removal in progress).
    closing: HashSet<String>,
}

impl ProjectLifecycleTracker {
    pub fn new() -> Self {
        Self::default()
    }

    // === creating ===

    pub fn mark_creating(&mut self, project_id: &str) {
        self.creating.insert(project_id.to_string());
    }

    pub fn finish_creating(&mut self, project_id: &str) {
        self.creating.remove(project_id);
    }

    pub fn is_creating(&self, project_id: &str) -> bool {
        self.creating.contains(project_id)
    }

    // === closing ===

    pub fn mark_closing(&mut self, project_id: &str) {
        self.closing.insert(project_id.to_string());
    }

    pub fn finish_closing(&mut self, project_id: &str) {
        self.closing.remove(project_id);
    }

    pub fn is_closing(&self, project_id: &str) -> bool {
        self.closing.contains(project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_lifecycle() {
        let mut tracker = ProjectLifecycleTracker::new();
        assert!(!tracker.is_creating("p1"));
        tracker.mark_creating("p1");
        assert!(tracker.is_creating("p1"));
        tracker.finish_creating("p1");
        assert!(!tracker.is_creating("p1"));
    }

    #[test]
    fn closing_lifecycle() {
        let mut tracker = ProjectLifecycleTracker::new();
        tracker.mark_closing("p1");
        assert!(tracker.is_closing("p1"));
        tracker.finish_closing("p1");
        assert!(!tracker.is_closing("p1"));
    }
}
