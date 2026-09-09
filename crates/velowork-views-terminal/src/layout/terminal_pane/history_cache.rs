//! In-memory project history cache for terminal autocompletion.
//!
//! Avoids executing synchronous SQLite queries on the UI thread during keystrokes,
//! providing sub-millisecond suggestions with prefix prioritization and frequency weighting.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use velowork_workspace::repositories::HistoryEntry;

pub const MAX_PROJECT_CACHE_ITEMS: usize = 300;
pub const MAX_SUGGESTION_RESULTS: usize = 6;

static GLOBAL_HISTORY_CACHE: OnceLock<Mutex<HashMap<String, Vec<HistoryEntry>>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<String, Vec<HistoryEntry>>> {
    GLOBAL_HISTORY_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct HistoryCache;

impl HistoryCache {
    /// Warm up or fetch cached entries for a project.
    /// If not yet cached, loads from SQLite on demand.
    pub fn get_or_load(project_id: &str) -> Vec<HistoryEntry> {
        let mut map = cache().lock().unwrap();
        if let Some(entries) = map.get(project_id) {
            return entries.clone();
        }

        let loaded = if let Some(db) = velowork_core::storage::database() {
            let repo = velowork_workspace::repositories::HistoryRepository::new(db);
            repo.list_by_project(project_id, None, MAX_PROJECT_CACHE_ITEMS)
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        map.insert(project_id.to_string(), loaded.clone());
        loaded
    }

    /// Record or update a command in the in-memory cache immediately.
    pub fn record_command(project_id: &str, cmd: &str) {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            return;
        }

        let mut map = cache().lock().unwrap();
        let list = map.entry(project_id.to_string()).or_insert_with(|| {
            if let Some(db) = velowork_core::storage::database() {
                let repo = velowork_workspace::repositories::HistoryRepository::new(db);
                repo.list_by_project(project_id, None, MAX_PROJECT_CACHE_ITEMS)
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        });

        let ts = velowork_workspace::repositories::now_iso8601();
        if let Some(pos) = list.iter().position(|e| e.command == trimmed) {
            let mut entry = list.remove(pos);
            entry.execution_count += 1;
            entry.timestamp = ts;
            list.insert(0, entry);
        } else {
            list.insert(
                0,
                HistoryEntry {
                    id: 0,
                    project_id: Some(project_id.to_string()),
                    profile_id: None,
                    session_id: None,
                    command: trimmed.to_string(),
                    cwd: None,
                    exit_code: None,
                    duration_ms: None,
                    execution_count: 1,
                    timestamp: ts,
                },
            );
        }

        if list.len() > MAX_PROJECT_CACHE_ITEMS {
            list.truncate(MAX_PROJECT_CACHE_ITEMS);
        }
    }

    /// Match suggestions from memory:
    /// 1. Prefix matches prioritized (case-insensitive)
    /// 2. Infix / substring matches secondary
    /// 3. Within each group: sorted by execution_count DESC, then recent timestamp/id DESC
    /// 4. Limited to `limit` entries (defaults to 6)
    pub fn match_suggestions(project_id: &str, query: &str, limit: usize) -> Vec<HistoryEntry> {
        let entries = Self::get_or_load(project_id);
        Self::filter_and_rank(&entries, query, limit.min(MAX_SUGGESTION_RESULTS))
    }

    /// Pure ranking function for testing and fast querying
    pub fn filter_and_rank(
        entries: &[HistoryEntry],
        query: &str,
        limit: usize,
    ) -> Vec<HistoryEntry> {
        let q = query.trim();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let q_lower = q.to_lowercase();

        let mut prefix_matches: Vec<HistoryEntry> = Vec::new();
        let mut infix_matches: Vec<HistoryEntry> = Vec::new();

        for entry in entries {
            let cmd_lower = entry.command.to_lowercase();
            if cmd_lower.starts_with(&q_lower) {
                prefix_matches.push(entry.clone());
            } else if cmd_lower.contains(&q_lower) {
                infix_matches.push(entry.clone());
            }
        }

        // Sort prefix matches: execution_count DESC, then timestamp DESC, then id DESC
        prefix_matches.sort_by(|a, b| {
            b.execution_count
                .cmp(&a.execution_count)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
                .then_with(|| b.id.cmp(&a.id))
        });

        // Sort infix matches: execution_count DESC, then timestamp DESC, then id DESC
        infix_matches.sort_by(|a, b| {
            b.execution_count
                .cmp(&a.execution_count)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
                .then_with(|| b.id.cmp(&a.id))
        });

        let mut results = prefix_matches;
        results.extend(infix_matches);
        results.truncate(limit);
        results
    }

    /// Invalidate cache for a project (e.g. when cleared)
    pub fn clear_project(project_id: &str) {
        let mut map = cache().lock().unwrap();
        map.remove(project_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: i64, command: &str, count: u32, timestamp: &str) -> HistoryEntry {
        HistoryEntry {
            id,
            project_id: Some("proj_1".into()),
            profile_id: None,
            session_id: None,
            command: command.to_string(),
            cwd: None,
            exit_code: None,
            duration_ms: None,
            execution_count: count,
            timestamp: timestamp.to_string(),
        }
    }

    #[test]
    fn test_prefix_prioritized_over_infix() {
        let entries = vec![
            make_entry(1, "docker login", 10, "2026-08-29T10:00:00Z"),
            make_entry(2, "git status", 2, "2026-08-29T10:05:00Z"),
            make_entry(3, "git commit", 5, "2026-08-29T10:10:00Z"),
            make_entry(4, "echo git", 20, "2026-08-29T10:15:00Z"),
        ];

        let results = HistoryCache::filter_and_rank(&entries, "git", 6);
        assert_eq!(results.len(), 3);
        // Prefix matches (git commit, git status) must come before infix match (echo git),
        // even though echo git has higher execution_count.
        assert_eq!(results[0].command, "git commit"); // count 5 prefix
        assert_eq!(results[1].command, "git status"); // count 2 prefix
        assert_eq!(results[2].command, "echo git"); // count 20 infix
    }

    #[test]
    fn test_frequency_ranking_within_prefix() {
        let entries = vec![
            make_entry(1, "cargo build", 1, "2026-08-29T10:00:00Z"),
            make_entry(2, "cargo test", 15, "2026-08-29T10:01:00Z"),
            make_entry(3, "cargo check", 8, "2026-08-29T10:02:00Z"),
        ];

        let results = HistoryCache::filter_and_rank(&entries, "cargo", 6);
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].command, "cargo test");
        assert_eq!(results[1].command, "cargo check");
        assert_eq!(results[2].command, "cargo build");
    }

    #[test]
    fn test_case_insensitivity() {
        let entries = vec![
            make_entry(1, "Git Push", 3, "2026-08-29T10:00:00Z"),
            make_entry(2, "git pull", 2, "2026-08-29T10:00:00Z"),
        ];

        let results = HistoryCache::filter_and_rank(&entries, "GIT", 6);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].command, "Git Push");
        assert_eq!(results[1].command, "git pull");
    }

    #[test]
    fn test_max_suggestion_limit() {
        let entries = (1..=20)
            .map(|i| make_entry(i, &format!("cmd_{}", i), i as u32, "2026-08-29T10:00:00Z"))
            .collect::<Vec<_>>();

        let results = HistoryCache::filter_and_rank(&entries, "cmd", 6);
        assert_eq!(results.len(), 6);
        // Top 6 highest counts
        assert_eq!(results[0].command, "cmd_20");
        assert_eq!(results[5].command, "cmd_15");
    }

    #[test]
    fn test_empty_query() {
        let entries = vec![make_entry(1, "ls", 1, "2026-08-29T10:00:00Z")];
        assert!(HistoryCache::filter_and_rank(&entries, "", 6).is_empty());
    }
}
