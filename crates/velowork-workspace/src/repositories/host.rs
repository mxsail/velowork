//! Host（远程 / SSH 主机）的 Repository → Domain → Application 分层。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use velowork_core::storage::{Database, rusqlite};

use super::now_iso8601;

/// 主机领域对象。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u32,
    pub username: Option<String>,
    pub auth_method: String, // password | key | agent
    pub credential_id: Option<String>,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Host {
    pub fn new(name: &str, host: &str) -> Self {
        let now = now_iso8601();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            host: host.to_string(),
            port: 22,
            username: None,
            auth_method: "password".to_string(),
            credential_id: None,
            group_id: None,
            tags: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// 行 ↔ 领域对象 的纯 CRUD 映射，不含业务规则。
pub struct HostRepository {
    db: Arc<Database>,
}

impl HostRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn insert(&self, host: &Host) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO host \
             (id, name, host, port, username, auth_method, credential_id, group_id, tags, created_at, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            rusqlite::params![
                host.id,
                host.name,
                host.host,
                host.port,
                host.username,
                host.auth_method,
                host.credential_id,
                host.group_id,
                serde_json::to_string(&host.tags).context("serialize host tags")?,
                host.created_at,
                host.updated_at,
            ],
        )
        .context("insert host")?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Host>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id,name,host,port,username,auth_method,credential_id,group_id,tags,created_at,updated_at \
                 FROM host WHERE id = ?",
            )
            .context("prepare host get")?;
        let mut rows = stmt.query([id]).context("query host get")?;
        match rows.next().context("iterate host get")? {
            Some(row) => Ok(Some(row_to_host(row)?)),
            None => Ok(None),
        }
    }

    pub fn update(&self, host: &Host) -> Result<()> {
        let conn = self.db.conn();
        let updated = now_iso8601();
        conn.execute(
            "UPDATE host SET name=?2, host=?3, port=?4, username=?5, auth_method=?6, \
             credential_id=?7, group_id=?8, tags=?9, updated_at=?10 WHERE id=?1",
            rusqlite::params![
                host.id,
                host.name,
                host.host,
                host.port,
                host.username,
                host.auth_method,
                host.credential_id,
                host.group_id,
                serde_json::to_string(&host.tags).context("serialize host tags")?,
                updated,
            ],
        )
        .context("update host")?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM host WHERE id = ?", [id])
            .context("delete host")?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Host>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id,name,host,port,username,auth_method,credential_id,group_id,tags,created_at,updated_at \
                 FROM host ORDER BY name",
            )
            .context("prepare host list")?;
        let rows = stmt
            .query_map([], |row| row_to_host(row))
            .context("query host list")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map host row")?);
        }
        Ok(out)
    }
}

fn row_to_host(row: &rusqlite::Row<'_>) -> rusqlite::Result<Host> {
    let tags_json: String = row.get(8)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Host {
        id: row.get(0)?,
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get(3)?,
        username: row.get(4)?,
        auth_method: row.get(5)?,
        credential_id: row.get(6)?,
        group_id: row.get(7)?,
        tags,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

/// 业务规则校验（无副作用）。
pub struct HostDomainService;

impl HostDomainService {
    pub fn validate(host: &Host) -> Result<()> {
        if host.name.trim().is_empty() {
            bail!("host name must not be empty");
        }
        if host.host.trim().is_empty() {
            bail!("host address must not be empty");
        }
        if host.port == 0 || host.port > 65535 {
            bail!("invalid port: {}", host.port);
        }
        Ok(())
    }
}

/// 用例编排：UI / 同步 / 导入只调本服务。
pub struct HostApplicationService {
    repo: HostRepository,
}

impl HostApplicationService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            repo: HostRepository::new(db),
        }
    }

    pub fn repo(&self) -> &HostRepository {
        &self.repo
    }

    /// 创建主机：校验 → 去重 → 落库。
    pub fn create(&self, host: Host) -> Result<Host> {
        HostDomainService::validate(&host)?;
        if self.repo.get(&host.id)?.is_some() {
            bail!("host already exists: {}", host.id);
        }
        self.repo.insert(&host)?;
        Ok(host)
    }

    /// 重命名（仅改 name，保持其他字段）。
    pub fn rename(&self, id: &str, new_name: &str) -> Result<Host> {
        let mut host = self
            .repo
            .get(id)?
            .with_context(|| format!("host not found: {id}"))?;
        host.name = new_name.to_string();
        HostDomainService::validate(&host)?;
        self.repo.update(&host)?;
        Ok(host)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.repo.delete(id)
    }

    pub fn list(&self) -> Result<Vec<Host>> {
        self.repo.list()
    }

    pub fn get(&self, id: &str) -> Result<Option<Host>> {
        self.repo.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> HostApplicationService {
        HostApplicationService::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn create_and_get_host() {
        let s = svc();
        let host = s.create(Host::new("web", "192.168.1.10")).unwrap();
        let fetched = s.get(&host.id).unwrap().unwrap();
        assert_eq!(fetched.name, "web");
        assert_eq!(fetched.host, "192.168.1.10");
        assert_eq!(fetched.port, 22);
    }

    #[test]
    fn rejects_empty_name() {
        let s = svc();
        let h = Host::new("", "1.2.3.4");
        // new() sets id; validation should fail on empty name.
        assert!(HostDomainService::validate(&h).is_err());
        // create also validates
        assert!(s.create(h).is_err());
    }

    #[test]
    fn rejects_bad_port() {
        let s = svc();
        let mut h = Host::new("x", "h");
        h.port = 70000;
        assert!(s.create(h).is_err());
    }

    #[test]
    fn rename_updates_name() {
        let s = svc();
        let host = s.create(Host::new("old", "h")).unwrap();
        let renamed = s.rename(&host.id, "new").unwrap();
        assert_eq!(renamed.name, "new");
    }

    #[test]
    fn delete_removes_host() {
        let s = svc();
        let host = s.create(Host::new("tmp", "h")).unwrap();
        s.delete(&host.id).unwrap();
        assert!(s.get(&host.id).unwrap().is_none());
    }

    #[test]
    fn list_returns_all() {
        let s = svc();
        s.create(Host::new("a", "h1")).unwrap();
        s.create(Host::new("b", "h2")).unwrap();
        assert_eq!(s.list().unwrap().len(), 2);
    }
}
