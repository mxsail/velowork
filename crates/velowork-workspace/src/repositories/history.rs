//! History（命令历史）的 Repository → Application 分层。

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use velowork_core::storage::{Database, rusqlite};

use super::now_iso8601;

use serde::{Deserialize, Serialize};

/// 命令历史条目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64, // 自增主键
    pub project_id: Option<String>,
    pub profile_id: Option<String>,
    pub session_id: Option<String>,
    pub command: String,
    pub cwd: Option<String>,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<i64>,
    pub execution_count: u32,
    pub timestamp: String,
}

/// 行 ↔ 领域对象 的纯 CRUD 映射。
pub struct HistoryRepository {
    db: Arc<Database>,
}

impl HistoryRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 记录项目命令历史（自动脱敏、去重、计数累加、置顶、超量与超时清理）
    pub fn record_project_command(
        &self,
        project_id: &str,
        command: &str,
        max_count: usize,
        retention_days: u32,
    ) -> Result<HistoryEntry> {
        let sanitized = velowork_core::security::SecretRedactor::redact_command(command);
        let cmd = sanitized.trim();
        if cmd.is_empty() {
            return Err(anyhow::anyhow!("command cannot be empty"));
        }

        let ts = now_iso8601();
        let conn = self.db.conn();

        // 1. 查找是否已存在该命令
        let existing: Option<(i64, u32)> = {
            let mut stmt = conn.prepare(
                "SELECT id, execution_count FROM history WHERE project_id = ? AND command = ? ORDER BY id DESC LIMIT 1"
            )?;
            let mut rows = stmt.query(rusqlite::params![project_id, cmd])?;
            if let Some(row) = rows.next()? {
                Some((row.get(0)?, row.get(1)?))
            } else {
                None
            }
        };

        let (id, count) = if let Some((_existing_id, existing_count)) = existing {
            // 删除旧行并重新插入，确保赋予最新的自增 ID 和时间戳（完全置顶）
            let _ = conn.execute(
                "DELETE FROM history WHERE project_id = ? AND command = ?",
                rusqlite::params![project_id, cmd],
            );

            conn.execute(
                "INSERT INTO history (project_id, command, execution_count, timestamp) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![project_id, cmd, existing_count + 1, ts],
            )
            .context("re-insert updated project history")?;
            let new_id = conn.last_insert_rowid();

            (new_id, existing_count + 1)
        } else {
            conn.execute(
                "INSERT INTO history (project_id, command, execution_count, timestamp) VALUES (?1, ?2, 1, ?3)",
                rusqlite::params![project_id, cmd, ts],
            )
            .context("insert project history")?;
            let new_id = conn.last_insert_rowid();
            (new_id, 1)
        };

        // 2. 超时保留天数清理 (retention_days > 0)
        if retention_days > 0 {
            let cutoff_dur = Duration::from_secs(retention_days as u64 * 86400);
            let cutoff_time = SystemTime::now().checked_sub(cutoff_dur).unwrap_or(SystemTime::UNIX_EPOCH);
            let cutoff_iso = chrono::DateTime::<chrono::Utc>::from(cutoff_time).to_rfc3339();
            let _ = conn.execute(
                "DELETE FROM history WHERE project_id = ? AND timestamp < ?",
                rusqlite::params![project_id, cutoff_iso],
            );
        }

        // 3. 超量最大条数裁剪 (max_count > 0)
        if max_count > 0 {
            let _ = conn.execute(
                "DELETE FROM history WHERE project_id = ? AND id NOT IN (
                    SELECT id FROM history WHERE project_id = ? ORDER BY timestamp DESC, id DESC LIMIT ?
                )",
                rusqlite::params![project_id, project_id, max_count],
            );
        }

        Ok(HistoryEntry {
            id,
            project_id: Some(project_id.to_string()),
            profile_id: None,
            session_id: None,
            command: cmd.to_string(),
            cwd: None,
            exit_code: None,
            duration_ms: None,
            execution_count: count,
            timestamp: ts,
        })
    }

    /// 按照项目查询命令历史列表（支持按关键词过滤）
    pub fn list_by_project(
        &self,
        project_id: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>> {
        let conn = self.db.conn();
        let mut out = Vec::new();
        let limit_i64 = limit as i64;

        if let Some(q) = query.filter(|s| !s.trim().is_empty()) {
            let pattern = format!("%{}%", q.trim());
            let mut stmt = conn.prepare(
                "SELECT id, project_id, profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp \
                 FROM history WHERE project_id = ? AND command LIKE ? ORDER BY timestamp DESC, id DESC LIMIT ?"
            ).context("prepare history list with query")?;

            let rows = stmt.query_map(rusqlite::params![project_id, pattern, limit_i64], |row| row_to_entry(row))?;
            for r in rows {
                out.push(r.context("map history row")?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp \
                 FROM history WHERE project_id = ? ORDER BY timestamp DESC, id DESC LIMIT ?"
            ).context("prepare history list")?;

            let rows = stmt.query_map(rusqlite::params![project_id, limit_i64], |row| row_to_entry(row))?;
            for r in rows {
                out.push(r.context("map history row")?);
            }
        }

        Ok(out)
    }

    /// 根据 ID 删除单条历史记录
    pub fn delete_by_id(&self, id: i64) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM history WHERE id = ?", rusqlite::params![id])
            .context("delete history by id")?;
        Ok(())
    }

    /// 清空指定项目的所有历史记录
    pub fn clear_by_project(&self, project_id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM history WHERE project_id = ?", rusqlite::params![project_id])
            .context("clear history by project")?;
        Ok(())
    }

    pub fn insert(
        &self,
        profile_id: Option<&str>,
        session_id: Option<&str>,
        command: &str,
        cwd: Option<&str>,
        exit_code: Option<i32>,
        duration_ms: Option<i64>,
    ) -> Result<HistoryEntry> {
        let sanitized = velowork_core::security::SecretRedactor::redact_command(command);
        let ts = now_iso8601();
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO history (profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp) \
             VALUES (?1,?2,?3,?4,?5,?6, 1, ?7)",
            rusqlite::params![profile_id, session_id, sanitized, cwd, exit_code, duration_ms, ts],
        )
        .context("insert history")?;
        let id = conn.last_insert_rowid();
        Ok(HistoryEntry {
            id,
            project_id: None,
            profile_id: profile_id.map(|s| s.to_string()),
            session_id: session_id.map(|s| s.to_string()),
            command: sanitized,
            cwd: cwd.map(|s| s.to_string()),
            exit_code,
            duration_ms,
            execution_count: 1,
            timestamp: ts,
        })
    }

    pub fn list(&self, profile_id: Option<&str>, limit: u32) -> Result<Vec<HistoryEntry>> {
        let conn = self.db.conn();
        let mut out = Vec::new();
        if let Some(pid) = profile_id {
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp \
                     FROM history WHERE profile_id = ? ORDER BY id DESC LIMIT ?",
                )
                .context("prepare history list by profile")?;
            let rows = stmt
                .query_map(rusqlite::params![pid, limit], |row| row_to_entry(row))
                .context("query history list by profile")?;
            for r in rows {
                out.push(r.context("map history row")?);
            }
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp \
                     FROM history ORDER BY id DESC LIMIT ?",
                )
                .context("prepare history list")?;
            let rows = stmt
                .query_map(rusqlite::params![limit], |row| row_to_entry(row))
                .context("query history list")?;
            for r in rows {
                out.push(r.context("map history row")?);
            }
        }
        Ok(out)
    }

    pub fn clear(&self, profile_id: Option<&str>) -> Result<()> {
        let conn = self.db.conn();
        if let Some(pid) = profile_id {
            conn.execute("DELETE FROM history WHERE profile_id = ?", [pid])
                .context("clear history by profile")?;
        } else {
            conn.execute("DELETE FROM history", [])
                .context("clear history")?;
        }
        Ok(())
    }

    /// 导出所有命令历史（供数据同步打包）
    pub fn export_rows(&self) -> Result<Vec<HistoryEntry>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp \
                 FROM history ORDER BY id ASC",
            )
            .context("prepare export history rows")?;
        let rows = stmt
            .query_map([], |row| row_to_entry(row))
            .context("query export history rows")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map export history row")?);
        }
        Ok(out)
    }

    /// 导入所有命令历史（供数据同步恢复与写入）
    pub fn import_rows(&self, entries: &[HistoryEntry]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin import history tx")?;
        tx.execute("DELETE FROM history", [])
            .context("clear history before import")?;
        for e in entries {
            tx.execute(
                "INSERT INTO history \
                 (id, project_id, profile_id, session_id, command, cwd, exit_code, duration_ms, execution_count, timestamp) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                rusqlite::params![
                    e.id,
                    e.project_id,
                    e.profile_id,
                    e.session_id,
                    e.command,
                    e.cwd,
                    e.exit_code,
                    e.duration_ms,
                    e.execution_count,
                    e.timestamp,
                ],
            )
            .context("insert imported history row")?;
        }
        tx.commit().context("commit import history tx")?;
        Ok(())
    }
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        profile_id: row.get(2)?,
        session_id: row.get(3)?,
        command: row.get(4)?,
        cwd: row.get(5)?,
        exit_code: row.get(6)?,
        duration_ms: row.get(7)?,
        execution_count: row.get::<_, Option<u32>>(8)?.unwrap_or(1),
        timestamp: row.get(9)?,
    })
}

/// 用例编排。
pub struct HistoryApplicationService {
    repo: HistoryRepository,
}

impl HistoryApplicationService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            repo: HistoryRepository::new(db),
        }
    }

    pub fn repo(&self) -> &HistoryRepository {
        &self.repo
    }

    pub fn record_project_command(
        &self,
        project_id: &str,
        command: &str,
        max_count: usize,
        retention_days: u32,
    ) -> Result<HistoryEntry> {
        self.repo.record_project_command(project_id, command, max_count, retention_days)
    }

    pub fn list_by_project(
        &self,
        project_id: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>> {
        self.repo.list_by_project(project_id, query, limit)
    }

    pub fn delete_by_id(&self, id: i64) -> Result<()> {
        self.repo.delete_by_id(id)
    }

    pub fn clear_by_project(&self, project_id: &str) -> Result<()> {
        self.repo.clear_by_project(project_id)
    }

    pub fn record(
        &self,
        profile_id: Option<&str>,
        session_id: Option<&str>,
        command: &str,
        cwd: Option<&str>,
        exit_code: Option<i32>,
        duration_ms: Option<i64>,
    ) -> Result<HistoryEntry> {
        self.repo
            .insert(profile_id, session_id, command, cwd, exit_code, duration_ms)
    }

    pub fn list(&self, profile_id: Option<&str>, limit: u32) -> Result<Vec<HistoryEntry>> {
        self.repo.list(profile_id, limit)
    }

    pub fn clear(&self, profile_id: Option<&str>) -> Result<()> {
        self.repo.clear(profile_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> HistoryApplicationService {
        HistoryApplicationService::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn record_and_list() {
        let s = svc();
        s.record(Some("default"), Some("sess1"), "ls", Some("/tmp"), Some(0), Some(5))
            .unwrap();
        s.record(Some("default"), Some("sess1"), "pwd", None, Some(0), Some(1))
            .unwrap();
        let list = s.list(Some("default"), 10).unwrap();
        assert_eq!(list.len(), 2);
        // 最新在前
        assert_eq!(list[0].command, "pwd");
    }

    #[test]
    fn project_history_dedup_and_count() {
        let s = svc();
        let p1 = "proj_1";
        s.record_project_command(p1, "git status", 100, 30).unwrap();
        s.record_project_command(p1, "cargo build", 100, 30).unwrap();
        s.record_project_command(p1, "git status", 100, 30).unwrap();

        let list = s.list_by_project(p1, None, 10).unwrap();
        assert_eq!(list.len(), 2);
        // 重复命令置顶
        assert_eq!(list[0].command, "git status");
        assert_eq!(list[0].execution_count, 2);
        assert_eq!(list[1].command, "cargo build");
        assert_eq!(list[1].execution_count, 1);
    }

    #[test]
    fn project_history_isolation() {
        let s = svc();
        s.record_project_command("proj_A", "npm start", 100, 30).unwrap();
        s.record_project_command("proj_B", "cargo run", 100, 30).unwrap();

        let list_a = s.list_by_project("proj_A", None, 10).unwrap();
        assert_eq!(list_a.len(), 1);
        assert_eq!(list_a[0].command, "npm start");

        let list_b = s.list_by_project("proj_B", None, 10).unwrap();
        assert_eq!(list_b.len(), 1);
        assert_eq!(list_b[0].command, "cargo run");
    }

    #[test]
    fn project_history_query_filter() {
        let s = svc();
        let p = "proj_filter";
        s.record_project_command(p, "docker ps", 100, 30).unwrap();
        s.record_project_command(p, "docker compose up", 100, 30).unwrap();
        s.record_project_command(p, "kubectl get pods", 100, 30).unwrap();

        let matches = s.list_by_project(p, Some("docker"), 10).unwrap();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn project_history_max_count_cleanup() {
        let s = svc();
        let p = "proj_limit";
        s.record_project_command(p, "cmd 1", 2, 30).unwrap();
        s.record_project_command(p, "cmd 2", 2, 30).unwrap();
        s.record_project_command(p, "cmd 3", 2, 30).unwrap();

        let list = s.list_by_project(p, None, 10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].command, "cmd 3");
        assert_eq!(list[1].command, "cmd 2");
    }

    #[test]
    fn project_history_multiline_command() {
        let s = svc();
        let p = "proj_multiline";
        let multiline_cmd = "df \\\n-lh";
        let entry = s.record_project_command(p, multiline_cmd, 100, 30).unwrap();
        assert_eq!(entry.command, multiline_cmd);

        let list = s.list_by_project(p, None, 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].command, multiline_cmd);
    }

    #[test]
    fn project_history_redacts_secrets() {
        let s = svc();
        let p = "proj_secrets";
        s.record_project_command(p, "mysql -u root -pMySecret123 -h localhost", 100, 30).unwrap();
        s.record_project_command(p, "curl -H \"Authorization: Bearer my_jwt_token\" https://api.com", 100, 30).unwrap();
        s.record_project_command(p, "psql postgres://admin:dbpass@localhost:5432/main", 100, 30).unwrap();

        let list = s.list_by_project(p, None, 10).unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].command, "psql postgres://admin:<redacted>@localhost:5432/main");
        assert_eq!(list[1].command, "curl -H \"Authorization: Bearer <redacted>\" https://api.com");
        assert_eq!(list[2].command, "mysql -u root -p<redacted> -h localhost");
    }
}
