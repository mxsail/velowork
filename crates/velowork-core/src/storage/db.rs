//! Single Profile business database wrapper.
//!
//! `Database` owns one `rusqlite::Connection` behind a `Mutex`. Opening applies
//! the WAL pragmas and runs all pending [`migrations`]. Writes from the
//! repository layer should be dispatched via `smol::unblock` so they never
//! block the GPUI main thread (see the repositories layer).
//!
//! [`migrations`]: crate::storage::migrations

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::storage::migrations::MIGRATIONS;

/// Process-wide handle to the active Profile's business database. Initialized
/// once at startup (after the v2 layout migration has created `data/`) via
/// [`init_database`]. Repository/Service layers read it through [`database`].
static ACTIVE_DB: OnceLock<Arc<Database>> = OnceLock::new();

/// Open the active Profile's `velowork.db`, run migrations, and register it as
/// the process-wide database. Must be called exactly once after the layout
/// migration. Returns the shared handle.
pub fn init_database(path: &Path) -> Result<Arc<Database>> {
    let db = Arc::new(Database::open(path).context("init profile database")?);
    ACTIVE_DB
        .set(db.clone())
        .map_err(|_| anyhow::anyhow!("database already initialized"))?;
    Ok(db)
}

/// Returns the active Profile's database handle, or `None` if startup has not
/// initialized it yet (e.g. tests, or a pre-migration code path).
pub fn database() -> Option<Arc<Database>> {
    ACTIVE_DB.get().cloned()
}

/// WAL pragmas applied once on open. WAL + NORMAL gives crash-safe, concurrent-
/// read/fast-write behavior suitable for an embedded app DB; foreign_keys=ON
/// keeps credential/host references consistent.
const WAL_PRAGMAS: &str = "
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
PRAGMA cache_size=-500;
";

/// A single SQLite database file for one Profile.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Open (creating if needed) the database at `path`, apply WAL pragmas and
    /// run all pending migrations. Parent directories are created.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating db dir {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        conn.execute_batch(WAL_PRAGMAS)
            .context("applying WAL pragmas")?;

        let db = Database {
            conn: Mutex::new(conn),
        };
        db.run_migrations()
            .context("running database migrations")?;
        Ok(db)
    }

    /// Lock and return the underlying connection. Callers run their own
    /// transactions; the lock is held only for the duration of the borrow.
    pub fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Current schema version (`PRAGMA user_version`).
    pub fn user_version(&self) -> Result<u32> {
        let conn = self.conn();
        let v: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .context("reading user_version")?;
        Ok(v)
    }

    /// Apply every migration whose version is greater than the current
    /// `user_version`, in order. Idempotent: re-running after a full migration
    /// is a no-op.
    pub fn run_migrations(&self) -> Result<()> {
        let current = self.user_version()?;
        let mut conn = self.conn();
        for m in MIGRATIONS {
            if m.version > current {
                log::info!(
                    "[storage] applying migration v{} ({})",
                    m.version,
                    m.name
                );
                let tx = conn.transaction()
                    .with_context(|| format!("begin tx for migration v{}", m.version))?;
                tx.execute_batch(m.up)
                    .with_context(|| format!("migration v{} ({})", m.version, m.name))?;
                tx.execute_batch(&format!("PRAGMA user_version = {}", m.version))
                    .with_context(|| format!("bumping user_version to {}", m.version))?;
                tx.commit()
                    .with_context(|| format!("commit migration v{}", m.version))?;
            }
        }
        Ok(())
    }
}

/// Convenience for tests / callers that want a throwaway in-memory database.
impl Database {
    /// Open an in-memory database (useful for tests and ephemeral profiles).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("opening in-memory database")?;
        conn.execute_batch(WAL_PRAGMAS)
            .context("applying WAL pragmas")?;
        let db = Database {
            conn: Mutex::new(conn),
        };
        db.run_migrations()?;
        Ok(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_memory_runs_migrations_and_sets_version() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as u32);

        // Every business + AI table from the init schema should exist.
        let tables = [
            "host", "session", "workspace", "history", "snippet", "folder",
            "favorite", "tag", "layout", "monitor_profile", "credential",
            "ai_context", "prompt_history", "workflow", "skill_cache",
            "terminal_snapshot", "selection_snapshot", "host_snapshot",
            "security_config", "security_state", "security_credentials",
            "tunnel_tree_node", "service_tree_node",
        ];
        let conn = db.conn();
        for t in tables {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?",
                    [t],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exists, "table {t} should exist after migration");
        }
    }

    #[test]
    fn migrations_are_idempotent() {
        let db = Database::open_in_memory().unwrap();
        // A second run_migrations must not error and must leave version at MIGRATIONS.len().
        db.run_migrations().unwrap();
        assert_eq!(db.user_version().unwrap(), MIGRATIONS.len() as u32);
    }

    #[test]
    fn can_insert_and_read_a_row() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO host (id, name, host, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            ("h1", "example", "example.com", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z"),
        )
        .unwrap();
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM host WHERE id = ?", ["h1"], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn init_database_registers_global_handle() {
        // 模拟启动流程：打开文件库并注册为进程级句柄，供 Repository 层读取。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles").join("default").join("data").join("velowork.db");
        let db = init_database(&path).expect("init_database");
        assert!(db.user_version().unwrap() >= 1);
        // 全局访问器返回同一实例。
        let global = database().expect("global database present");
        assert!(Arc::ptr_eq(&db, &global));
    }
}
