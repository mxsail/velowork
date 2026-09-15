//! AI 数据访问：独立于 Workspace 的 `AiRepository` / `AiService`。
//!
//! 表：`prompt_history`（对话历史）、`workflow`（工作流定义）、
//! `skill_cache`（技能缓存）、`ai_context`（上下文片段）。这些 AI 预留表与
//! 业务表同库但逻辑独立，便于未来单独演进。

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use velowork_core::storage::{Database, rusqlite};

use super::now_iso8601;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 一条对话历史记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptRecord {
    pub id: String,
    pub model_id: Option<String>,
    pub role: String, // system | user | assistant
    pub content: String,
    pub tokens: Option<u32>,
    pub created_at: String,
}

fn default_revision() -> u64 {
    1
}

/// AI 会话数据库行。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConversationRow {
    pub id: String,
    pub profile_id: Option<String>,
    pub project_id: Option<String>,
    pub title: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub status: String,       // active | archive | deleted
    pub context_mode: String, // manual | session | workspace | temporary
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "default_revision")]
    pub revision: u64,
    #[serde(default)]
    pub device_id: String,
}

/// AI 消息数据库行。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiMessageRow {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub token_count: Option<u32>,
    pub metadata: String, // JSON
    pub created_at: String,
    #[serde(default = "default_revision")]
    pub revision: u64,
    #[serde(default)]
    pub device_id: String,
}

/// AI 上下文快照数据库行。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiContextRow {
    pub id: String,
    pub conversation_id: String,
    pub type_: String, // ssh_session | workspace | file | terminal | manual
    pub target_id: Option<String>,
    pub snapshot: String, // JSON
    pub created_at: String,
}

/// AI 记忆条目数据库行。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiMemoryRow {
    pub id: String,
    pub profile_id: Option<String>,
    pub scope: String, // global | workspace | session | project
    pub content: String,
    pub embedding: Option<Vec<u8>>,
    pub created_at: String,
}

/// AI 附件数据库行。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiAttachmentRow {
    pub id: String,
    pub message_id: String,
    pub path: String,
    pub size: u64,
    pub hash: Option<String>,
    pub created_at: String,
}

/// AI 会话检索结果（包含会话行以及可选的高亮摘要上下文片段）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiConversationSearchResult {
    pub conversation: AiConversationRow,
    pub matched_snippet: Option<String>,
}

/// 安全地从文本中提取包含关键词的前后上下文片段（UTF-8 字符边界安全，杜绝切片 Panic）。
pub fn safe_extract_snippet(text: &str, query: &str, max_chars_around: usize) -> Option<String> {
    if text.is_empty() || query.is_empty() {
        return None;
    }
    let lower_text = text.to_lowercase();
    let lower_query = query.to_lowercase();
    let byte_pos = lower_text.find(&lower_query)?;

    let char_indices: Vec<(usize, char)> = text.char_indices().collect();
    let match_char_idx = char_indices.iter().position(|&(b, _)| b >= byte_pos).unwrap_or(0);

    let start_char_idx = match_char_idx.saturating_sub(max_chars_around);
    let end_char_idx = (match_char_idx + query.chars().count() + max_chars_around).min(char_indices.len());

    let start_byte = char_indices.get(start_char_idx).map(|&(b, _)| b).unwrap_or(0);
    let end_byte = if end_char_idx >= char_indices.len() {
        text.len()
    } else {
        char_indices.get(end_char_idx).map(|&(b, _)| b).unwrap_or(text.len())
    };

    let mut snippet = String::new();
    if start_char_idx > 0 {
        snippet.push_str("...");
    }
    let raw_slice = &text[start_byte..end_byte];
    let sanitized: String = raw_slice
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    snippet.push_str(sanitized.trim());
    if end_char_idx < char_indices.len() {
        snippet.push_str("...");
    }

    Some(snippet)
}

/// 工作流定义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workflow {
    pub id: String,
    pub name: Option<String>,
    pub definition: String, // JSON
    pub created_at: String,
    pub updated_at: String,
}

/// 技能缓存条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillCacheEntry {
    pub id: String,
    pub skill: String,
    pub input_hash: String,
    pub output: Option<String>,
    pub created_at: String,
    pub expires_at: Option<String>, // Unix 秒（文本）；空 = 不过期
}

/// AI 数据的纯 CRUD 映射。
pub struct AiRepository {
    db: Arc<Database>,
}

impl AiRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    // --- Relational AI Conversation & Message CRUD ---

    pub fn save_conversation(&self, conv: &AiConversationRow) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO ai_conversations (id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
             ON CONFLICT(id) DO UPDATE SET \
               profile_id=excluded.profile_id, project_id=excluded.project_id, title=excluded.title, \
               provider_id=excluded.provider_id, model=excluded.model, status=excluded.status, \
               context_mode=excluded.context_mode, updated_at=excluded.updated_at",
            rusqlite::params![
                conv.id, conv.profile_id, conv.project_id, conv.title, conv.provider_id,
                conv.model, conv.status, conv.context_mode, conv.created_at, conv.updated_at
            ],
        )
        .context("save ai_conversation")?;
        Ok(())
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<AiConversationRow>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id FROM ai_conversations WHERE id = ?1")
            .context("prepare get_conversation")?;
        let mut rows = stmt.query_map([id], |row| {
            Ok(AiConversationRow {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                project_id: row.get(2)?,
                title: row.get(3)?,
                provider_id: row.get(4)?,
                model: row.get(5)?,
                status: row.get(6)?,
                context_mode: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                revision: row.get::<_, i64>(10).map(|v| v as u64).unwrap_or(1),
                device_id: row.get(11).unwrap_or_default(),
            })
        })?;
        if let Some(r) = rows.next() {
            Ok(Some(r.context("map ai_conversation row")?))
        } else {
            Ok(None)
        }
    }

    fn map_conversation_row(row: &rusqlite::Row) -> rusqlite::Result<AiConversationRow> {
        Ok(AiConversationRow {
            id: row.get(0)?,
            profile_id: row.get(1)?,
            project_id: row.get(2)?,
            title: row.get(3)?,
            provider_id: row.get(4)?,
            model: row.get(5)?,
            status: row.get(6)?,
            context_mode: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            revision: row.get::<_, i64>(10).map(|v| v as u64).unwrap_or(1),
            device_id: row.get(11).unwrap_or_default(),
        })
    }

    pub fn list_conversations(&self, project_id: Option<&str>) -> Result<Vec<AiConversationRow>> {
        let conn = self.db.conn();
        let mut out = Vec::new();
        if let Some(pid) = project_id {
            let mut stmt = conn.prepare(
                "SELECT id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id \
                 FROM ai_conversations WHERE project_id = ?1 AND status != 'deleted' ORDER BY updated_at DESC"
            )?;
            let rows = stmt.query_map([pid], Self::map_conversation_row)?;
            for r in rows {
                out.push(r.context("map ai_conversation row")?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id \
                 FROM ai_conversations WHERE status != 'deleted' ORDER BY updated_at DESC"
            )?;
            let rows = stmt.query_map([], Self::map_conversation_row)?;
            for r in rows {
                out.push(r.context("map ai_conversation row")?);
            }
        }
        Ok(out)
    }

    /// 搜索历史会话。
    /// - `search_content = false`：仅在会话标题中进行 LIKE 模糊搜索；
    /// - `search_content = true`：联合检索会话标题与消息正文，并在命中的正文中提取上下文片段。
    pub fn search_conversations(
        &self,
        project_id: Option<&str>,
        query: &str,
        search_content: bool,
    ) -> Result<Vec<AiConversationSearchResult>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            let convs = self.list_conversations(project_id)?;
            return Ok(convs
                .into_iter()
                .map(|c| AiConversationSearchResult {
                    conversation: c,
                    matched_snippet: None,
                })
                .collect());
        }

        let conn = self.db.conn();
        let pattern = format!("%{}%", trimmed);

        if !search_content {
            let mut out = Vec::new();
            if let Some(pid) = project_id {
                let mut stmt = conn.prepare(
                    "SELECT id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id \
                     FROM ai_conversations WHERE project_id = ?1 AND status != 'deleted' AND title LIKE ?2 ORDER BY updated_at DESC LIMIT 50"
                )?;
                let rows = stmt.query_map(rusqlite::params![pid, pattern], Self::map_conversation_row)?;
                for r in rows {
                    out.push(AiConversationSearchResult {
                        conversation: r.context("map ai_conversation row")?,
                        matched_snippet: None,
                    });
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id \
                     FROM ai_conversations WHERE status != 'deleted' AND title LIKE ?1 ORDER BY updated_at DESC LIMIT 50"
                )?;
                let rows = stmt.query_map(rusqlite::params![pattern], Self::map_conversation_row)?;
                for r in rows {
                    out.push(AiConversationSearchResult {
                        conversation: r.context("map ai_conversation row")?,
                        matched_snippet: None,
                    });
                }
            }
            return Ok(out);
        }

        // 全文消息正文联合检索
        let mut results = Vec::new();
        let mut seen_ids = std::collections::HashSet::new();

        // 1. 先查标题直接命中的会话
        let title_matches = self.search_conversations(project_id, query, false)?;
        for item in title_matches {
            seen_ids.insert(item.conversation.id.clone());
            results.push(item);
        }

        // 2. 查 ai_messages 中正文命中的会话
        let mut stmt = if project_id.is_some() {
            conn.prepare(
                "SELECT c.id, c.profile_id, c.project_id, c.title, c.provider_id, c.model, c.status, c.context_mode, c.created_at, c.updated_at, c.revision, c.device_id, m.content \
                 FROM ai_messages m \
                 JOIN ai_conversations c ON m.conversation_id = c.id \
                 WHERE c.project_id = ?1 AND c.status != 'deleted' AND m.content LIKE ?2 \
                 ORDER BY m.rowid DESC LIMIT 100"
            )?
        } else {
            conn.prepare(
                "SELECT c.id, c.profile_id, c.project_id, c.title, c.provider_id, c.model, c.status, c.context_mode, c.created_at, c.updated_at, c.revision, c.device_id, m.content \
                 FROM ai_messages m \
                 JOIN ai_conversations c ON m.conversation_id = c.id \
                 WHERE c.status != 'deleted' AND m.content LIKE ?1 \
                 ORDER BY m.rowid DESC LIMIT 100"
            )?
        };

        fn map_conv_content(row: &rusqlite::Row) -> rusqlite::Result<(AiConversationRow, String)> {
            let conv = AiRepository::map_conversation_row(row)?;
            let content: String = row.get(12)?;
            Ok((conv, content))
        }

        let rows = if let Some(pid) = project_id {
            stmt.query_map(rusqlite::params![pid, pattern], map_conv_content)?
        } else {
            stmt.query_map(rusqlite::params![pattern], map_conv_content)?
        };

        for r in rows {
            let (conv, content) = r.context("query message row")?;
            if seen_ids.insert(conv.id.clone()) {
                let snippet = safe_extract_snippet(&content, trimmed, 30);
                results.push(AiConversationSearchResult {
                    conversation: conv,
                    matched_snippet: snippet,
                });
                if results.len() >= 50 {
                    break;
                }
            }
        }

        Ok(results)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM ai_conversations WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    pub fn save_message(&self, msg: &AiMessageRow) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO ai_messages (id, conversation_id, role, content, token_count, metadata, created_at, revision, device_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(id) DO UPDATE SET role=excluded.role, content=excluded.content, token_count=excluded.token_count, metadata=excluded.metadata",
            rusqlite::params![
                msg.id, msg.conversation_id, msg.role, msg.content, msg.token_count, msg.metadata, msg.created_at,
                msg.revision as i64, msg.device_id
            ],
        )?;
        Ok(())
    }

    pub fn list_messages(&self, conversation_id: &str) -> Result<Vec<AiMessageRow>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, token_count, metadata, created_at, revision, device_id \
             FROM ai_messages WHERE conversation_id = ?1 ORDER BY rowid DESC LIMIT 100"
        )?;
        let rows = stmt.query_map([conversation_id], |row| {
            Ok(AiMessageRow {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                token_count: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
                revision: row.get::<_, i64>(7).map(|v| v as u64).unwrap_or(1),
                device_id: row.get(8).unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map ai_message row")?);
        }
        out.reverse();
        Ok(out)
    }

    /// 分页加载指定会话的消息（按时间正序返回），同时返回本批次最早消息的 rowid 以及是否还有更多。
    /// - `before_rowid`: 若为 `Some(rid)`，仅查询 `rowid < rid` 的更早消息；若为 `None`，从最新消息开始取。
    /// - `limit`: 查询条数上限。
    /// 返回值：`(messages, oldest_rowid, has_more)`
    pub fn list_messages_paged(
        &self,
        conversation_id: &str,
        before_rowid: Option<i64>,
        limit: usize,
    ) -> Result<(Vec<AiMessageRow>, Option<i64>, bool)> {
        let conn = self.db.conn();
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<(AiMessageRow, i64)> {
            Ok((
                AiMessageRow {
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    token_count: row.get(4)?,
                    metadata: row.get(5)?,
                    created_at: row.get(6)?,
                    revision: row.get::<_, i64>(7).map(|v| v as u64).unwrap_or(1),
                    device_id: row.get(8).unwrap_or_default(),
                },
                row.get::<_, i64>(9)?,
            ))
        };

        let mut items = Vec::new();
        if let Some(before_id) = before_rowid {
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, role, content, token_count, metadata, created_at, revision, device_id, rowid \
                 FROM ai_messages WHERE conversation_id = ?1 AND rowid < ?2 ORDER BY rowid DESC LIMIT ?3"
            )?;
            let rows = stmt.query_map(rusqlite::params![conversation_id, before_id, (limit + 1) as i64], map_row)?;
            for r in rows {
                items.push(r.context("map ai_message row with rowid")?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id, role, content, token_count, metadata, created_at, revision, device_id, rowid \
                 FROM ai_messages WHERE conversation_id = ?1 ORDER BY rowid DESC LIMIT ?2"
            )?;
            let rows = stmt.query_map(rusqlite::params![conversation_id, (limit + 1) as i64], map_row)?;
            for r in rows {
                items.push(r.context("map ai_message row with rowid")?);
            }
        }

        let has_more = items.len() > limit;
        if has_more {
            items.truncate(limit);
        }

        let oldest_rowid = items.last().map(|(_, rid)| *rid);
        let mut messages: Vec<AiMessageRow> = items.into_iter().map(|(msg, _)| msg).collect();
        messages.reverse();
        Ok((messages, oldest_rowid, has_more))
    }

    /// 获取会话消息总数
    pub fn count_messages(&self, conversation_id: &str) -> Result<usize> {
        let conn = self.db.conn();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM ai_messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn delete_messages(&self, conversation_id: &str) -> Result<()> {
        let conn = self.db.conn();
        conn.execute("DELETE FROM ai_messages WHERE conversation_id = ?1", rusqlite::params![conversation_id])?;
        Ok(())
    }

    /// 导出所有 AI 会话（供数据同步打包）
    pub fn export_conversations(&self) -> Result<Vec<AiConversationRow>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id \
             FROM ai_conversations ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AiConversationRow {
                id: row.get(0)?,
                profile_id: row.get(1)?,
                project_id: row.get(2)?,
                title: row.get(3)?,
                provider_id: row.get(4)?,
                model: row.get(5)?,
                status: row.get(6)?,
                context_mode: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                revision: row.get::<_, i64>(10).map(|v| v as u64).unwrap_or(1),
                device_id: row.get(11).unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map export ai_conversation row")?);
        }
        Ok(out)
    }

    /// 导出所有 AI 消息（供数据同步打包）
    pub fn export_messages(&self) -> Result<Vec<AiMessageRow>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, token_count, metadata, created_at, revision, device_id \
             FROM ai_messages ORDER BY rowid ASC"
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(AiMessageRow {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                token_count: row.get(4)?,
                metadata: row.get(5)?,
                created_at: row.get(6)?,
                revision: row.get::<_, i64>(7).map(|v| v as u64).unwrap_or(1),
                device_id: row.get(8).unwrap_or_default(),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map export ai_message row")?);
        }
        Ok(out)
    }

    /// 导入 AI 会话（供数据同步恢复与写入）
    pub fn import_conversations(&self, rows: &[AiConversationRow]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin import ai_conversations tx")?;
        tx.execute("DELETE FROM ai_conversations", [])
            .context("clear ai_conversations before import")?;
        for conv in rows {
            tx.execute(
                "INSERT INTO ai_conversations \
                 (id, profile_id, project_id, title, provider_id, model, status, context_mode, created_at, updated_at, revision, device_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                rusqlite::params![
                    conv.id, conv.profile_id, conv.project_id, conv.title, conv.provider_id,
                    conv.model, conv.status, conv.context_mode, conv.created_at, conv.updated_at,
                    conv.revision as i64, conv.device_id
                ],
            )?;
        }
        tx.commit().context("commit import ai_conversations tx")?;
        Ok(())
    }

    /// 导入 AI 消息（供数据同步恢复与写入）
    pub fn import_messages(&self, rows: &[AiMessageRow]) -> Result<()> {
        let mut conn = self.db.conn();
        let tx = conn.transaction().context("begin import ai_messages tx")?;
        tx.execute("DELETE FROM ai_messages", [])
            .context("clear ai_messages before import")?;
        for msg in rows {
            tx.execute(
                "INSERT INTO ai_messages \
                 (id, conversation_id, role, content, token_count, metadata, created_at, revision, device_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    msg.id, msg.conversation_id, msg.role, msg.content, msg.token_count, msg.metadata, msg.created_at,
                    msg.revision as i64, msg.device_id
                ],
            )?;
        }
        tx.commit().context("commit import ai_messages tx")?;
        Ok(())
    }

    pub fn save_context_snapshot(&self, ctx: &AiContextRow) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO ai_contexts (id, conversation_id, type, target_id, snapshot, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![ctx.id, ctx.conversation_id, ctx.type_, ctx.target_id, ctx.snapshot, ctx.created_at],
        )?;
        Ok(())
    }

    pub fn get_latest_context_snapshot(&self, conversation_id: &str) -> Result<Option<AiContextRow>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, type, target_id, snapshot, created_at \
             FROM ai_contexts WHERE conversation_id = ?1 ORDER BY created_at DESC LIMIT 1"
        )?;
        let mut rows = stmt.query_map([conversation_id], |row| {
            Ok(AiContextRow {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                type_: row.get(2)?,
                target_id: row.get(3)?,
                snapshot: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        if let Some(r) = rows.next() {
            Ok(Some(r.context("map ai_context row")?))
        } else {
            Ok(None)
        }
    }

    pub fn save_memory(&self, mem: &AiMemoryRow) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO ai_memory (id, profile_id, scope, content, embedding, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![mem.id, mem.profile_id, mem.scope, mem.content, mem.embedding, mem.created_at],
        )?;
        Ok(())
    }

    pub fn list_memories(&self, scope: Option<&str>) -> Result<Vec<AiMemoryRow>> {
        let conn = self.db.conn();
        let mut out = Vec::new();
        if let Some(s) = scope {
            let mut stmt = conn.prepare(
                "SELECT id, profile_id, scope, content, embedding, created_at \
                 FROM ai_memory WHERE scope = ?1 ORDER BY created_at DESC"
            )?;
            let rows = stmt.query_map([s], |row| {
                Ok(AiMemoryRow {
                    id: row.get(0)?,
                    profile_id: row.get(1)?,
                    scope: row.get(2)?,
                    content: row.get(3)?,
                    embedding: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?;
            for r in rows {
                out.push(r.context("map ai_memory row")?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, profile_id, scope, content, embedding, created_at \
                 FROM ai_memory ORDER BY created_at DESC"
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(AiMemoryRow {
                    id: row.get(0)?,
                    profile_id: row.get(1)?,
                    scope: row.get(2)?,
                    content: row.get(3)?,
                    embedding: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?;
            for r in rows {
                out.push(r.context("map ai_memory row")?);
            }
        }
        Ok(out)
    }

    pub fn save_attachment(&self, att: &AiAttachmentRow) -> Result<()> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO ai_attachments (id, message_id, path, size, hash, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![att.id, att.message_id, att.path, att.size as i64, att.hash, att.created_at],
        )?;
        Ok(())
    }

    pub fn list_attachments(&self, message_id: &str) -> Result<Vec<AiAttachmentRow>> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, message_id, path, size, hash, created_at \
             FROM ai_attachments WHERE message_id = ?1 ORDER BY created_at ASC"
        )?;
        let rows = stmt.query_map([message_id], |row| {
            let sz: i64 = row.get(3)?;
            Ok(AiAttachmentRow {
                id: row.get(0)?,
                message_id: row.get(1)?,
                path: row.get(2)?,
                size: sz as u64,
                hash: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map ai_attachment row")?);
        }
        Ok(out)
    }

    pub fn log_prompt(
        &self,
        model_id: Option<&str>,
        role: &str,
        content: &str,
        tokens: Option<u32>,
    ) -> Result<PromptRecord> {
        let rec = PromptRecord {
            id: uuid::Uuid::new_v4().to_string(),
            model_id: model_id.map(|s| s.to_string()),
            role: role.to_string(),
            content: content.to_string(),
            tokens,
            created_at: now_iso8601(),
        };
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO prompt_history (id, model_id, role, content, tokens, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                rec.id, rec.model_id, rec.role, rec.content, rec.tokens, rec.created_at
            ],
        )
        .context("insert prompt_history")?;
        Ok(rec)
    }

    pub fn prompt_history(&self, limit: u32) -> Result<Vec<PromptRecord>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, model_id, role, content, tokens, created_at FROM prompt_history \
                 ORDER BY rowid DESC LIMIT ?",
            )
            .context("prepare prompt_history list")?;
        let rows = stmt
            .query_map([limit], |row| {
                Ok(PromptRecord {
                    id: row.get(0)?,
                    model_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    tokens: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .context("query prompt_history list")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map prompt row")?);
        }
        Ok(out)
    }

    pub fn save_workflow(&self, name: Option<&str>, definition: &str) -> Result<Workflow> {
        let now = now_iso8601();
        let wf = Workflow {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.map(|s| s.to_string()),
            definition: definition.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO workflow (id, name, definition, created_at, updated_at) \
             VALUES (?1,?2,?3,?4,?5)",
            rusqlite::params![wf.id, wf.name, wf.definition, wf.created_at, wf.updated_at],
        )
        .context("insert workflow")?;
        Ok(wf)
    }

    pub fn list_workflows(&self) -> Result<Vec<Workflow>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare("SELECT id, name, definition, created_at, updated_at FROM workflow ORDER BY name")
            .context("prepare workflow list")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Workflow {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    definition: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            })
            .context("query workflow list")?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.context("map workflow row")?);
        }
        Ok(out)
    }

    pub fn cache_skill(
        &self,
        skill: &str,
        input_hash: &str,
        output: Option<&str>,
        ttl_secs: Option<i64>,
    ) -> Result<SkillCacheEntry> {
        let expires_at = ttl_secs.map(|ttl| (now_secs() + ttl).to_string());
        let entry = SkillCacheEntry {
            id: uuid::Uuid::new_v4().to_string(),
            skill: skill.to_string(),
            input_hash: input_hash.to_string(),
            output: output.map(|s| s.to_string()),
            created_at: now_iso8601(),
            expires_at,
        };
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO skill_cache (id, skill, input_hash, output, created_at, expires_at) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            rusqlite::params![
                entry.id, entry.skill, entry.input_hash, entry.output, entry.created_at, entry.expires_at
            ],
        )
        .context("insert skill_cache")?;
        Ok(entry)
    }

    /// 命中缓存则返回 output；过期或缺失返回 None。
    pub fn get_cached_skill(&self, skill: &str, input_hash: &str) -> Result<Option<String>> {
        let conn = self.db.conn();
        let mut stmt = conn
            .prepare(
                "SELECT output, expires_at FROM skill_cache WHERE skill = ? AND input_hash = ? \
                 ORDER BY created_at DESC LIMIT 1",
            )
            .context("prepare skill_cache get")?;
        let mut rows = stmt.query([skill, input_hash]).context("query skill_cache get")?;
        match rows.next().context("iterate skill_cache get")? {
            Some(row) => {
                let output: Option<String> = row.get(0)?;
                let expires_at: Option<String> = row.get(1)?;
                if let Some(exp) = expires_at {
                    if let Ok(exp_secs) = exp.parse::<i64>() {
                        if exp_secs <= now_secs() {
                            return Ok(None); // expired
                        }
                    }
                }
                Ok(output)
            }
            None => Ok(None),
        }
    }
}

/// AI 用例编排（独立于 Workspace 业务）。
pub struct AiService {
    repo: AiRepository,
}

impl AiService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            repo: AiRepository::new(db),
        }
    }

    pub fn repo(&self) -> &AiRepository {
        &self.repo
    }

    pub fn save_conversation(&self, conv: &AiConversationRow) -> Result<()> {
        self.repo.save_conversation(conv)
    }

    pub fn get_conversation(&self, id: &str) -> Result<Option<AiConversationRow>> {
        self.repo.get_conversation(id)
    }

    pub fn list_conversations(&self, project_id: Option<&str>) -> Result<Vec<AiConversationRow>> {
        self.repo.list_conversations(project_id)
    }

    pub fn delete_conversation(&self, id: &str) -> Result<()> {
        self.repo.delete_conversation(id)
    }

    pub fn search_conversations(
        &self,
        project_id: Option<&str>,
        query: &str,
        search_content: bool,
    ) -> Result<Vec<AiConversationSearchResult>> {
        self.repo.search_conversations(project_id, query, search_content)
    }

    pub fn save_message(&self, msg: &AiMessageRow) -> Result<()> {
        self.repo.save_message(msg)
    }

    pub fn list_messages(&self, conversation_id: &str) -> Result<Vec<AiMessageRow>> {
        self.repo.list_messages(conversation_id)
    }

    pub fn list_messages_paged(
        &self,
        conversation_id: &str,
        before_rowid: Option<i64>,
        limit: usize,
    ) -> Result<(Vec<AiMessageRow>, Option<i64>, bool)> {
        self.repo.list_messages_paged(conversation_id, before_rowid, limit)
    }

    pub fn count_messages(&self, conversation_id: &str) -> Result<usize> {
        self.repo.count_messages(conversation_id)
    }

    pub fn delete_messages(&self, conversation_id: &str) -> Result<()> {
        self.repo.delete_messages(conversation_id)
    }

    pub fn save_context_snapshot(&self, ctx: &AiContextRow) -> Result<()> {
        self.repo.save_context_snapshot(ctx)
    }

    pub fn get_latest_context_snapshot(&self, conversation_id: &str) -> Result<Option<AiContextRow>> {
        self.repo.get_latest_context_snapshot(conversation_id)
    }

    pub fn save_memory(&self, mem: &AiMemoryRow) -> Result<()> {
        self.repo.save_memory(mem)
    }

    pub fn list_memories(&self, scope: Option<&str>) -> Result<Vec<AiMemoryRow>> {
        self.repo.list_memories(scope)
    }

    pub fn save_attachment(&self, att: &AiAttachmentRow) -> Result<()> {
        self.repo.save_attachment(att)
    }

    pub fn list_attachments(&self, message_id: &str) -> Result<Vec<AiAttachmentRow>> {
        self.repo.list_attachments(message_id)
    }

    pub fn log_prompt(
        &self,
        model_id: Option<&str>,
        role: &str,
        content: &str,
        tokens: Option<u32>,
    ) -> Result<PromptRecord> {
        self.repo.log_prompt(model_id, role, content, tokens)
    }

    pub fn prompt_history(&self, limit: u32) -> Result<Vec<PromptRecord>> {
        self.repo.prompt_history(limit)
    }

    pub fn save_workflow(&self, name: Option<&str>, definition: &str) -> Result<Workflow> {
        self.repo.save_workflow(name, definition)
    }

    pub fn list_workflows(&self) -> Result<Vec<Workflow>> {
        self.repo.list_workflows()
    }

    pub fn cache_skill(
        &self,
        skill: &str,
        input_hash: &str,
        output: Option<&str>,
        ttl_secs: Option<i64>,
    ) -> Result<SkillCacheEntry> {
        self.repo.cache_skill(skill, input_hash, output, ttl_secs)
    }

    pub fn get_cached_skill(&self, skill: &str, input_hash: &str) -> Result<Option<String>> {
        self.repo.get_cached_skill(skill, input_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> AiService {
        AiService::new(Arc::new(Database::open_in_memory().unwrap()))
    }

    #[test]
    fn log_and_read_prompt_history() {
        let s = svc();
        s.log_prompt(Some("gpt-4o"), "user", "hello", Some(10)).unwrap();
        s.log_prompt(Some("gpt-4o"), "assistant", "hi", Some(20))
            .unwrap();
        let history = s.prompt_history(10).unwrap();
        assert_eq!(history.len(), 2);
        // 最新在前
        assert_eq!(history[0].role, "assistant");
    }

    #[test]
    fn save_and_list_workflows() {
        let s = svc();
        s.save_workflow(Some("deploy"), "{}").unwrap();
        assert_eq!(s.list_workflows().unwrap().len(), 1);
    }

    #[test]
    fn skill_cache_hit_and_miss() {
        let s = svc();
        s.cache_skill("summarize", "hash1", Some("result"), None)
            .unwrap();
        assert_eq!(
            s.get_cached_skill("summarize", "hash1").unwrap(),
            Some("result".to_string())
        );
        assert_eq!(s.get_cached_skill("summarize", "unknown").unwrap(), None);
    }

    #[test]
    fn skill_cache_expires() {
        let s = svc();
        // ttl = 0 → already expired
        s.cache_skill("s", "h", Some("out"), Some(0)).unwrap();
        assert_eq!(s.get_cached_skill("s", "h").unwrap(), None);
    }

    #[test]
    fn conversation_message_and_context_snapshot() {
        let s = svc();
        let conv = AiConversationRow {
            id: "conv-1".into(),
            profile_id: Some("default".into()),
            project_id: Some("proj-123".into()),
            title: Some("Fix docker".into()),
            provider_id: Some("deepseek".into()),
            model: Some("deepseek-chat".into()),
            status: "active".into(),
            context_mode: "session".into(),
            created_at: "2026-08-07T00:00:00Z".into(),
            updated_at: "2026-08-07T00:00:00Z".into(),
            revision: 1,
            device_id: "".into(),
        };
        s.save_conversation(&conv).unwrap();

        let fetched = s.get_conversation("conv-1").unwrap().unwrap();
        assert_eq!(fetched.title.as_deref(), Some("Fix docker"));
        assert_eq!(fetched.project_id.as_deref(), Some("proj-123"));

        let msg = AiMessageRow {
            id: "msg-1".into(),
            conversation_id: "conv-1".into(),
            role: "user".into(),
            content: "Why did nginx crash?".into(),
            token_count: Some(15),
            metadata: "{}".into(),
            created_at: "2026-08-07T00:01:00Z".into(),
            revision: 1,
            device_id: "".into(),
        };
        s.save_message(&msg).unwrap();

        let msgs = s.list_messages("conv-1").unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "Why did nginx crash?");

        let ctx = AiContextRow {
            id: "ctx-1".into(),
            conversation_id: "conv-1".into(),
            type_: "ssh_session".into(),
            target_id: Some("sess-88".into()),
            snapshot: r#"{"hostname":"10.0.0.5","user":"root"}"#.into(),
            created_at: "2026-08-07T00:01:01Z".into(),
        };
        s.save_context_snapshot(&ctx).unwrap();

        let snap = s.get_latest_context_snapshot("conv-1").unwrap().unwrap();
        assert_eq!(snap.type_, "ssh_session");
        assert_eq!(snap.target_id.as_deref(), Some("sess-88"));

        let mem = AiMemoryRow {
            id: "mem-1".into(),
            profile_id: Some("default".into()),
            scope: "session".into(),
            content: "Production runs Ubuntu 22.04".into(),
            embedding: None,
            created_at: "2026-08-07T00:02:00Z".into(),
        };
        s.save_memory(&mem).unwrap();
        let memories = s.list_memories(Some("session")).unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].content, "Production runs Ubuntu 22.04");
    }

    #[test]
    fn test_list_messages_paged_and_count() {
        let s = svc();
        let conv = AiConversationRow {
            id: "conv-page".into(),
            profile_id: Some("default".into()),
            project_id: Some("p1".into()),
            title: Some("Paging Test".into()),
            provider_id: None,
            model: None,
            status: "active".into(),
            context_mode: "session".into(),
            created_at: "2026-08-07T00:00:00Z".into(),
            updated_at: "2026-08-07T00:00:00Z".into(),
            revision: 1,
            device_id: "".into(),
        };
        s.save_conversation(&conv).unwrap();

        // 插入 25 条消息
        for i in 1..=25 {
            let msg = AiMessageRow {
                id: format!("msg-{}", i),
                conversation_id: "conv-page".into(),
                role: if i % 2 == 1 { "user".into() } else { "assistant".into() },
                content: format!("Message {}", i),
                token_count: Some(10),
                metadata: "{}".into(),
                created_at: format!("2026-08-07T00:{:02}:00Z", i),
                revision: 1,
                device_id: "".into(),
            };
            s.save_message(&msg).unwrap();
        }

        assert_eq!(s.count_messages("conv-page").unwrap(), 25);

        // 首屏：取最新的 10 条（即 16..=25，正序排列）
        let (page1, oldest_rowid, has_more) = s.list_messages_paged("conv-page", None, 10).unwrap();
        assert_eq!(page1.len(), 10);
        assert!(has_more);
        assert_eq!(page1[0].content, "Message 16");
        assert_eq!(page1[9].content, "Message 25");
        assert!(oldest_rowid.is_some());

        // 第二页：取最早 rowid 之前的 10 条（即 6..=15）
        let (page2, oldest_rowid2, has_more2) = s.list_messages_paged("conv-page", oldest_rowid, 10).unwrap();
        assert_eq!(page2.len(), 10);
        assert!(has_more2);
        assert_eq!(page2[0].content, "Message 6");
        assert_eq!(page2[9].content, "Message 15");

        // 第三页：剩余 5 条（即 1..=5）
        let (page3, _oldest_rowid3, has_more3) = s.list_messages_paged("conv-page", oldest_rowid2, 10).unwrap();
        assert_eq!(page3.len(), 5);
        assert!(!has_more3);
        assert_eq!(page3[0].content, "Message 1");
        assert_eq!(page3[4].content, "Message 5");
    }

    #[test]
    fn test_search_conversations_title_and_content() {
        let s = svc();
        let conv1 = AiConversationRow {
            id: "conv-search-1".into(),
            profile_id: Some("default".into()),
            project_id: Some("proj-alpha".into()),
            title: Some("Docker 容器网络排查".into()),
            provider_id: None,
            model: None,
            status: "active".into(),
            context_mode: "session".into(),
            created_at: "2026-08-07T00:00:00Z".into(),
            updated_at: "2026-08-07T00:00:00Z".into(),
            revision: 1,
            device_id: "".into(),
        };
        let conv2 = AiConversationRow {
            id: "conv-search-2".into(),
            profile_id: Some("default".into()),
            project_id: Some("proj-alpha".into()),
            title: Some("常规前端开发记录".into()),
            provider_id: None,
            model: None,
            status: "active".into(),
            context_mode: "session".into(),
            created_at: "2026-08-07T00:01:00Z".into(),
            updated_at: "2026-08-07T00:01:00Z".into(),
            revision: 1,
            device_id: "".into(),
        };
        s.save_conversation(&conv1).unwrap();
        s.save_conversation(&conv2).unwrap();

        let msg2 = AiMessageRow {
            id: "msg-nested-1".into(),
            conversation_id: "conv-search-2".into(),
            role: "assistant".into(),
            content: "在配置中遇到了一个跨域 CORS error 错误，需要修改代理设置。".into(),
            token_count: Some(20),
            metadata: "{}".into(),
            created_at: "2026-08-07T00:02:00Z".into(),
            revision: 1,
            device_id: "".into(),
        };
        s.save_message(&msg2).unwrap();

        // 1. 纯标题搜索：搜 "Docker" 能搜出 conv1
        let title_res = s.search_conversations(Some("proj-alpha"), "Docker", false).unwrap();
        assert_eq!(title_res.len(), 1);
        assert_eq!(title_res[0].conversation.id, "conv-search-1");
        assert!(title_res[0].matched_snippet.is_none());

        // 2. 纯标题搜索：搜 "CORS" 搜不出任何结果（因为标题不含 CORS）
        let title_none = s.search_conversations(Some("proj-alpha"), "CORS", false).unwrap();
        assert_eq!(title_none.len(), 0);

        // 3. 全文检索：开启 search_content 后，搜 "CORS" 成功搜出 conv2 并带有匹配摘要
        let content_res = s.search_conversations(Some("proj-alpha"), "CORS", true).unwrap();
        assert_eq!(content_res.len(), 1);
        assert_eq!(content_res[0].conversation.id, "conv-search-2");
        assert!(content_res[0].matched_snippet.is_some());
        let snip = content_res[0].matched_snippet.as_ref().unwrap();
        assert!(snip.contains("CORS"));
    }

    #[test]
    fn test_safe_extract_snippet() {
        use super::safe_extract_snippet;
        let text = "这是一段很长很长的文本，包含关键字 Velowork 终端应用，后面还有很多字符。";
        let snip = safe_extract_snippet(text, "Velowork", 5);
        assert!(snip.is_some());
        let s = snip.unwrap();
        assert!(s.contains("Velowork"));
        assert!(s.starts_with("..."));
        assert!(s.ends_with("..."));
    }
}
