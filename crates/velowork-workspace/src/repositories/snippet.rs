//! Snippet（代码片段 / 快捷指令）的 Repository → Domain → Application 分层。

use std::sync::Arc;

use anyhow::{Context, Result, bail};
use velowork_core::storage::{Database, rusqlite};

use super::now_iso8601;

/// 代码片段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub content: String,
    pub language: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Snippet {
    pub fn new(name: &str, content: &str) -> Self {
        let now = now_iso8601();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            content: content.to_string(),
            language: None,
            tags: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// 行 ↔ 领域对象 的纯 CRUD 映射。
pub struct SnippetRepository {
    db: Arc<Database>,
}

impl SnippetRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub fn insert(&self, s: &Snippet) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO snippet (id, name, content, language, tags, created_at, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![
                s.id,
                s.name,
                s.content,
                s.language,
                serde_json::to_string(&s.tags).context("serialize snippet tags")?,
                s.created_at,
                s.updated_at,
            ],
        )
        .context("insert snippet")?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Snippet>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, language, tags, created_at, updated_at FROM snippet WHERE id = ?",
            )
            .context("prepare snippet get")?;
        let mut rows = stmt.query([id]).context("query snippet get")?;
        match rows.next().context("iterate snippet get")? {
            Some(row) => Ok(Some(row_to_snippet(row)?)),
            None => Ok(None),
        }
    }

    pub fn update(&self, s: &Snippet) -> Result<()> {
        let conn = self.db.conn();
        let updated = now_iso8601();
        conn.execute(
            "UPDATE snippet SET name=?2, content=?3, language=?4, tags=?5, updated_at=?6 WHERE id=?1",
            rusqlite::params![
                s.id, s.name, s.content, s.language,
                serde_json::to_string(&s.tags).context("serialize snippet tags")?, updated,
            ],
        )
        .context("update snippet")?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM snippet WHERE id = ?", [id])
            .context("delete snippet")?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Snippet>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, content, language, tags, created_at, updated_at FROM snippet ORDER BY name",
            )
            .context("prepare snippet list")?;
        let rows = stmt
            .query_map([], |row| row_to_snippet(row))
            .context("query snippet list")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map snippet row")?);
        }
        Ok(out)
    }
}

fn row_to_snippet(row: &rusqlite::Row<'_>) -> rusqlite::Result<Snippet> {
    let tags_json: String = row.get(4)?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Snippet {
        id: row.get(0)?,
        name: row.get(1)?,
        content: row.get(2)?,
        language: row.get(3)?,
        tags,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// 业务规则校验。
pub struct SnippetDomainService;

impl SnippetDomainService {
    pub fn validate(s: &Snippet) -> Result<()> {
        if s.name.trim().is_empty() {
            bail!("snippet name must not be empty");
        }
        if s.content.trim().is_empty() {
            bail!("snippet content must not be empty");
        }
        Ok(())
    }
}

/// 用例编排。
pub struct SnippetApplicationService {
    repo: SnippetRepository,
}

impl SnippetApplicationService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            repo: SnippetRepository::new(db),
        }
    }

    pub fn repo(&self) -> &SnippetRepository {
        &self.repo
    }

    pub fn create(&self, s: Snippet) -> Result<Snippet> {
        SnippetDomainService::validate(&s)?;
        if self.repo.get(&s.id)?.is_some() {
            bail!("snippet already exists: {}", s.id);
        }
        self.repo.insert(&s)?;
        Ok(s)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        self.repo.delete(id)
    }

    pub fn list(&self) -> Result<Vec<Snippet>> {
        self.repo.list()
    }

    pub fn get(&self, id: &str) -> Result<Option<Snippet>> {
        self.repo.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> SnippetApplicationService {
        SnippetApplicationService::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn create_and_list() {
        let s = svc();
        s.create(Snippet::new("deploy", "kubectl apply -f")).unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn rejects_empty_content() {
        let s = svc();
        let snippet = Snippet::new("x", "");
        assert!(s.create(snippet).is_err());
    }

    #[test]
    fn delete_removes() {
        let s = svc();
        let snippet = s.create(Snippet::new("tmp", "echo")).unwrap();
        s.delete(&snippet.id).unwrap();
        assert!(s.get(&snippet.id).unwrap().is_none());
    }
}
