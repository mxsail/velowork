//! Session（终端 / Shell 会话，已晋升为 DB 实体）的 Repository → Domain →
//! Application 分层。
//!
//! 注意：`sessions/` 目录仅保留 `*.sshconfig` / `*.pem` / `import/`（连接配置与
//! 密钥文件），而会话的元数据与运行状态统一落在 `velowork.db` 的 `session` 表，
//! 避免 SQLite + JSON 双套维护。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use velowork_core::storage::{Database, rusqlite};

use super::now_iso8601;

/// 会话领域对象。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub profile_id: Option<String>,
    pub name: Option<String>,
    pub kind: String, // local | ssh | serial | ...
    pub host_id: Option<String>,
    pub backend: Option<String>, // tmux | screen | none
    pub backend_session_id: Option<String>,
    pub cwd: Option<String>,
    pub shell: Option<String>,
    pub status: Option<String>,
    pub data: Option<String>, // JSON blob of extra state
    pub created_at: String,
    pub updated_at: String,
    pub last_used_at: Option<String>,
}

impl Session {
    pub fn new(kind: &str) -> Self {
        let now = now_iso8601();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: None,
            name: None,
            kind: kind.to_string(),
            host_id: None,
            backend: None,
            backend_session_id: None,
            cwd: None,
            shell: None,
            status: Some("active".to_string()),
            data: None,
            created_at: now.clone(),
            updated_at: now,
            last_used_at: None,
        }
    }
}

/// 行 ↔ 领域对象 的纯 CRUD 映射。
pub struct SessionRepository {
    db: Arc<Database>,
}

impl SessionRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn insert(&self, s: &Session) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO session \
             (id, profile_id, name, kind, host_id, backend, backend_session_id, cwd, shell, status, data, created_at, updated_at, last_used_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            rusqlite::params![
                s.id, s.profile_id, s.name, s.kind, s.host_id, s.backend,
                s.backend_session_id, s.cwd, s.shell, s.status, s.data,
                s.created_at, s.updated_at, s.last_used_at,
            ],
        )
        .context("insert session")?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Session>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id,profile_id,name,kind,host_id,backend,backend_session_id,cwd,shell,status,data,created_at,updated_at,last_used_at \
                 FROM session WHERE id = ?",
            )
            .context("prepare session get")?;
        let mut rows = stmt.query([id]).context("query session get")?;
        match rows.next().context("iterate session get")? {
            Some(row) => Ok(Some(row_to_session(row)?)),
            None => Ok(None),
        }
    }

    pub fn update(&self, s: &Session) -> Result<()> {
        let conn = self.db.conn();
        let updated = now_iso8601();
        conn.execute(
            "UPDATE session SET profile_id=?2, name=?3, kind=?4, host_id=?5, backend=?6, \
             backend_session_id=?7, cwd=?8, shell=?9, status=?10, data=?11, updated_at=?12, last_used_at=?13 \
             WHERE id=?1",
            rusqlite::params![
                s.id, s.profile_id, s.name, s.kind, s.host_id, s.backend,
                s.backend_session_id, s.cwd, s.shell, s.status, s.data, updated, s.last_used_at,
            ],
        )
        .context("update session")?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM session WHERE id = ?", [id])
            .context("delete session")?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Session>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id,profile_id,name,kind,host_id,backend,backend_session_id,cwd,shell,status,data,created_at,updated_at,last_used_at \
                 FROM session ORDER BY last_used_at DESC, created_at DESC",
            )
            .context("prepare session list")?;
        let rows = stmt
            .query_map([], |row| row_to_session(row))
            .context("query session list")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map session row")?);
        }
        Ok(out)
    }

    pub fn list_by_host(&self, host_id: &str) -> Result<Vec<Session>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|s| s.host_id.as_deref() == Some(host_id))
            .collect())
    }
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        profile_id: row.get(1)?,
        name: row.get(2)?,
        kind: row.get(3)?,
        host_id: row.get(4)?,
        backend: row.get(5)?,
        backend_session_id: row.get(6)?,
        cwd: row.get(7)?,
        shell: row.get(8)?,
        status: row.get(9)?,
        data: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        last_used_at: row.get(13)?,
    })
}

/// 业务规则校验。
pub struct SessionDomainService;

impl SessionDomainService {
    pub fn validate(s: &Session) -> Result<()> {
        if s.kind.trim().is_empty() {
            bail!("session kind must not be empty");
        }
        Ok(())
    }
}

/// 用例编排。
pub struct SessionApplicationService {
    repo: SessionRepository,
}

impl SessionApplicationService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            repo: SessionRepository::new(db),
        }
    }

    pub fn repo(&self) -> &SessionRepository {
        &self.repo
    }

    pub fn create(&self, s: Session) -> Result<Session> {
        SessionDomainService::validate(&s)?;
        if self.repo.get(&s.id)?.is_some() {
            bail!("session already exists: {}", s.id);
        }
        self.repo.insert(&s)?;
        Ok(s)
    }

    /// 更新最后使用时间（用于重连排序）。
    pub fn touch(&self, id: &str) -> Result<()> {
        let mut s = self
            .repo
            .get(id)?
            .with_context(|| format!("session not found: {id}"))?;
        s.last_used_at = Some(now_iso8601());
        s.status = Some("active".to_string());
        self.repo.update(&s)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.repo.delete(id)
    }

    pub fn list(&self) -> Result<Vec<Session>> {
        self.repo.list()
    }

    pub fn get(&self, id: &str) -> Result<Option<Session>> {
        self.repo.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> SessionApplicationService {
        SessionApplicationService::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn create_and_list_session() {
        let s = svc();
        let sess = s.create(Session::new("local")).unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
        assert_eq!(s.get(&sess.id).unwrap().unwrap().kind, "local");
    }

    #[test]
    fn touch_updates_last_used() {
        let s = svc();
        let sess = s.create(Session::new("ssh")).unwrap();
        s.touch(&sess.id).unwrap();
        let fetched = s.get(&sess.id).unwrap().unwrap();
        assert!(fetched.last_used_at.is_some());
        assert_eq!(fetched.status.as_deref(), Some("active"));
    }

    #[test]
    fn delete_session() {
        let s = svc();
        let sess = s.create(Session::new("local")).unwrap();
        s.delete(&sess.id).unwrap();
        assert!(s.get(&sess.id).unwrap().is_none());
    }

    #[test]
    fn list_by_host_filters() {
        let s = svc();
        let mut a = Session::new("ssh");
        a.host_id = Some("host-1".to_string());
        let mut b = Session::new("ssh");
        b.host_id = Some("host-2".to_string());
        s.create(a).unwrap();
        s.create(b).unwrap();
        assert_eq!(s.repo().list_by_host("host-1").unwrap().len(), 1);
    }
}
