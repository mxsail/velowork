//! Unified SQLite schema for the single Profile business database
//! (`data/velowork.db`).
//!
//! All business tables live in one database so a Profile stays a single,
//! self-contained, syncable unit. Credential *secrets* never touch this file —
//! only credential **metadata** (the secret itself lives in the OS keyring);
//! see `credential` below.
//!
//! Migrations are versioned and applied in order via `PRAGMA user_version`.
//! Add new entries to [`MIGRATIONS`] rather than editing existing SQL, so
//! existing databases upgrade forward without data loss.

/// A single forward migration.
pub struct Migration {
    /// Monotonic schema version. Must be > 0 and strictly increasing.
    pub version: u32,
    /// Human-readable name (logged when applied).
    pub name: &'static str,
    /// SQL applied to reach this version (idempotent `CREATE TABLE IF NOT EXISTS`).
    pub up: &'static str,
}

/// 当前数据库 schema 版本（`PRAGMA user_version`）。与 `MIGRATIONS` 末项一致。
pub const CURRENT_SCHEMA_VERSION: u32 = 10;

/// Initial schema (user_version = 1).
pub const INIT_SCHEMA: &str = r#"
-- Remote / SSH hosts.
CREATE TABLE IF NOT EXISTS host (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    host          TEXT NOT NULL,
    port          INTEGER NOT NULL DEFAULT 22,
    username      TEXT,
    auth_method   TEXT,            -- password | key | agent
    credential_id TEXT,            -- FK -> credential.id (secret in keyring)
    group_id      TEXT,
    tags          TEXT,            -- JSON array of tag ids
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_host_group ON host(group_id);

-- Terminal / shell sessions (promoted to a first-class DB entity).
CREATE TABLE IF NOT EXISTS session (
    id                TEXT PRIMARY KEY,
    profile_id        TEXT,
    name              TEXT,
    kind              TEXT NOT NULL,  -- local | ssh | serial | ...
    host_id           TEXT,            -- for remote sessions -> host.id
    backend           TEXT,            -- tmux | screen | none
    backend_session_id TEXT,           -- e.g. tmux session name
    cwd               TEXT,
    shell             TEXT,
    status            TEXT,            -- active | exited | ...
    data              TEXT,            -- JSON blob of extra state
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    last_used_at      TEXT
);
CREATE INDEX IF NOT EXISTS idx_session_host ON session(host_id);
CREATE INDEX IF NOT EXISTS idx_session_profile ON session(profile_id);

-- Persisted workspace state (multi-workspace / snapshot support).
CREATE TABLE IF NOT EXISTS workspace (
    id         TEXT PRIMARY KEY,
    name       TEXT,
    data       TEXT NOT NULL,      -- JSON of WorkspaceData
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Command history.
CREATE TABLE IF NOT EXISTS history (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  TEXT,
    session_id  TEXT,
    command     TEXT NOT NULL,
    cwd         TEXT,
    exit_code   INTEGER,
    duration_ms INTEGER,
    timestamp   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_history_session ON history(session_id);
CREATE INDEX IF NOT EXISTS idx_history_ts ON history(timestamp);

-- Snippets / quick commands.
CREATE TABLE IF NOT EXISTS snippet (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    content    TEXT NOT NULL,
    language   TEXT,
    tags       TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Project folders.
CREATE TABLE IF NOT EXISTS folder (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    parent_id   TEXT,
    color       TEXT,
    collapsed   INTEGER DEFAULT 0,
    order_index INTEGER,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Favorites (projects / hosts / commands).
CREATE TABLE IF NOT EXISTS favorite (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,   -- project | host | command
    target_id  TEXT,
    name       TEXT,
    created_at TEXT NOT NULL
);

-- Tags.
CREATE TABLE IF NOT EXISTS tag (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    color      TEXT,
    created_at TEXT NOT NULL
);

-- Saved layouts.
CREATE TABLE IF NOT EXISTS layout (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    data       TEXT NOT NULL,   -- JSON layout tree
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Resource monitor profiles.
CREATE TABLE IF NOT EXISTS monitor_profile (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    config     TEXT NOT NULL,   -- JSON config
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Credential METADATA only. The secret lives in the OS keyring, addressed by
-- `id` (the account string "<profile>:<type>:<id>").
CREATE TABLE IF NOT EXISTS credential (
    id          TEXT PRIMARY KEY,
    profile_id  TEXT NOT NULL,
    kind        TEXT NOT NULL,  -- password | api_key | ssh_key | token | ...
    name        TEXT,
    provider    TEXT,           -- keyring | encrypted_file | memory
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    last_used_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_credential_profile ON credential(profile_id);

-- ===== AI reserved tables (populated by AiRepository / AiService) =====

CREATE TABLE IF NOT EXISTS ai_context (
    id         TEXT PRIMARY KEY,
    session_id TEXT,
    content    TEXT,
    tokens     INTEGER,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_context_session ON ai_context(session_id);

CREATE TABLE IF NOT EXISTS prompt_history (
    id         TEXT PRIMARY KEY,
    model_id   TEXT,
    role       TEXT,            -- system | user | assistant
    content    TEXT,
    tokens     INTEGER,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_prompt_history_model ON prompt_history(model_id);

CREATE TABLE IF NOT EXISTS workflow (
    id         TEXT PRIMARY KEY,
    name       TEXT,
    definition TEXT,            -- JSON
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS skill_cache (
    id         TEXT PRIMARY KEY,
    skill      TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    output     TEXT,
    created_at TEXT NOT NULL,
    expires_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_skill_cache_skill ON skill_cache(skill, input_hash);

CREATE TABLE IF NOT EXISTS terminal_snapshot (
    id         TEXT PRIMARY KEY,
    session_id TEXT,
    content    TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_terminal_snapshot_session ON terminal_snapshot(session_id);

CREATE TABLE IF NOT EXISTS selection_snapshot (
    id         TEXT PRIMARY KEY,
    session_id TEXT,
    content    TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_selection_snapshot_session ON selection_snapshot(session_id);

CREATE TABLE IF NOT EXISTS host_snapshot (
    id         TEXT PRIMARY KEY,
    host_id    TEXT,
    metrics    TEXT,            -- JSON
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_host_snapshot_host ON host_snapshot(host_id);
"#;

/// Schema for the SSH session connection tree (`session_tree_node`).
///
/// Replaces the legacy `ssh_sessions.json` flat file. The recursive tree is
/// flattened into rows keyed by `parent_id` + `sort_index` so it lives alongside
/// the other business tables in `velowork.db`.
pub const SESSION_TREE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS session_tree_node (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,            -- 'folder' | 'session'
    parent_id    TEXT,                     -- NULL = root level
    sort_index   INTEGER NOT NULL DEFAULT 0,
    name         TEXT NOT NULL,
    is_collapsed INTEGER NOT NULL DEFAULT 0,
    payload      TEXT,                     -- SshSession JSON (session nodes only)
    updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_session_tree_parent ON session_tree_node(parent_id);
"#;

/// Ordered list of all migrations. Append new versions; never reorder or edit
/// existing entries (that would break already-migrated databases).
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "init-schema",
        up: INIT_SCHEMA,
    },
    Migration {
        version: 2,
        name: "add-session-tree",
        up: SESSION_TREE_SCHEMA,
    },
    Migration {
        version: 3,
        name: "add-session-tree-project-id",
        up: ADD_SESSION_TREE_PROJECT_ID,
    },
    Migration {
        version: 4,
        name: "add-security-schema",
        up: SECURITY_SCHEMA,
    },
    Migration {
        version: 5,
        name: "add-security-state-cooldown-until-unix",
        up: "ALTER TABLE security_state ADD COLUMN cooldown_until_unix INTEGER NOT NULL DEFAULT 0;",
    },
    Migration {
        version: 6,
        name: "add-ai-relational-schema",
        up: AI_RELATIONAL_SCHEMA,
    },
    Migration {
        version: 7,
        name: "add-history-project-id",
        up: ADD_HISTORY_PROJECT_ID,
    },
    Migration {
        version: 8,
        name: "add-tunnel-service-tree",
        up: TUNNEL_SERVICE_TREE_SCHEMA,
    },
    Migration {
        version: 9,
        name: "add-workspace-table",
        up: WORKSPACE_SCHEMA,
    },
    Migration {
        version: 10,
        name: "add-sync-revision-columns",
        up: ADD_SYNC_REVISION_COLUMNS,
    },
];

pub const ADD_SYNC_REVISION_COLUMNS: &str = r#"
ALTER TABLE session_tree_node ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE session_tree_node ADD COLUMN device_id TEXT NOT NULL DEFAULT '';
ALTER TABLE tunnel_tree_node ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE tunnel_tree_node ADD COLUMN device_id TEXT NOT NULL DEFAULT '';
ALTER TABLE service_tree_node ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE service_tree_node ADD COLUMN device_id TEXT NOT NULL DEFAULT '';
ALTER TABLE ai_conversations ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE ai_conversations ADD COLUMN device_id TEXT NOT NULL DEFAULT '';
ALTER TABLE ai_messages ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
ALTER TABLE ai_messages ADD COLUMN device_id TEXT NOT NULL DEFAULT '';
"#;

/// Workspace data table for storing single-project/multi-project layout state in SQLite.
pub const WORKSPACE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS workspace (
    id         TEXT PRIMARY KEY,
    name       TEXT,
    data       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
"#;

/// Tunnel and service monitor trees (邻接表模式，与 `session_tree_node` 对齐)。
///
/// Replaces `tunnels.json` and `service_monitors.json`. Each recursive tree is
/// flattened into rows keyed by `parent_id` + `sort_index`.
pub const TUNNEL_SERVICE_TREE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tunnel_tree_node (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,            -- 'folder' | 'tunnel'
    parent_id    TEXT,                     -- NULL = root level
    sort_index   INTEGER NOT NULL DEFAULT 0,
    name         TEXT NOT NULL,
    is_expanded  INTEGER NOT NULL DEFAULT 1,
    project_id   TEXT,
    payload      TEXT,                     -- TunnelProfile JSON (tunnel nodes only)
    updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tunnel_tree_parent ON tunnel_tree_node(parent_id);
CREATE INDEX IF NOT EXISTS idx_tunnel_tree_project ON tunnel_tree_node(project_id);

CREATE TABLE IF NOT EXISTS service_tree_node (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,            -- 'folder' | 'service'
    parent_id    TEXT,                     -- NULL = root level
    sort_index   INTEGER NOT NULL DEFAULT 0,
    name         TEXT NOT NULL,
    is_expanded  INTEGER NOT NULL DEFAULT 1,
    project_id   TEXT,
    payload      TEXT,                     -- ServiceDefinition JSON (service nodes only)
    updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_service_tree_parent ON service_tree_node(parent_id);
CREATE INDEX IF NOT EXISTS idx_service_tree_project ON service_tree_node(project_id);
"#;

pub const ADD_HISTORY_PROJECT_ID: &str = r#"
ALTER TABLE history ADD COLUMN project_id TEXT DEFAULT NULL;
ALTER TABLE history ADD COLUMN execution_count INTEGER NOT NULL DEFAULT 1;
CREATE INDEX IF NOT EXISTS idx_history_project ON history(project_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_history_project_cmd ON history(project_id, command);
"#;

pub const AI_RELATIONAL_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS ai_conversations (
    id            TEXT PRIMARY KEY,
    profile_id    TEXT,
    project_id    TEXT,
    title         TEXT,
    provider_id   TEXT,
    model         TEXT,
    status        TEXT NOT NULL DEFAULT 'active',
    context_mode  TEXT NOT NULL DEFAULT 'session',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_conversations_project ON ai_conversations(project_id);

CREATE TABLE IF NOT EXISTS ai_messages (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role            TEXT NOT NULL,
    content         TEXT NOT NULL,
    token_count     INTEGER,
    metadata        TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES ai_conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_ai_messages_conv ON ai_messages(conversation_id);

CREATE TABLE IF NOT EXISTS ai_contexts (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    type            TEXT NOT NULL,
    target_id       TEXT,
    snapshot        TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL,
    FOREIGN KEY(conversation_id) REFERENCES ai_conversations(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_ai_contexts_conv ON ai_contexts(conversation_id);

CREATE TABLE IF NOT EXISTS ai_memory (
    id          TEXT PRIMARY KEY,
    profile_id  TEXT,
    scope       TEXT NOT NULL,
    content     TEXT NOT NULL,
    embedding   BLOB,
    created_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ai_memory_scope ON ai_memory(scope);

CREATE TABLE IF NOT EXISTS ai_attachments (
    id          TEXT PRIMARY KEY,
    message_id  TEXT NOT NULL,
    path        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    hash        TEXT,
    created_at  TEXT NOT NULL,
    FOREIGN KEY(message_id) REFERENCES ai_messages(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_ai_attachments_msg ON ai_attachments(message_id);
"#;

/// Add `project_id` column to `session_tree_node` for per-project data isolation.
/// Existing rows with NULL project_id are treated as the default project's data.
pub const ADD_SESSION_TREE_PROJECT_ID: &str = r#"
ALTER TABLE session_tree_node ADD COLUMN project_id TEXT DEFAULT NULL;
CREATE INDEX IF NOT EXISTS idx_session_tree_project ON session_tree_node(project_id);
"#;

/// 分层安全架构（v4）：安全配置 / 安全状态 / 通用 Secret Store（密文落 SQLite）。
///
/// - `security_config`：安全模式、KDF 参数、salt、verifier、加密后的 DEK（Enhanced 模式）。
/// - `security_state`：迁移/解锁/轮换等运行时状态（与静态配置解耦）。
/// - `security_credentials`：通用 Secret Store，密文（nonce|ciphertext）落库，DEK 保护；
///   算法升级 Repository 零改动（`version` + `algorithm_id` 驱动 `decrypt` 分支）。
pub const SECURITY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS security_config (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    mode            INTEGER NOT NULL DEFAULT 0,   -- 0=Standard 1=Enhanced 2=Maximum(预留)
    kdf_algorithm   TEXT NOT NULL DEFAULT 'Argon2id',
    kdf_parameters  TEXT NOT NULL DEFAULT '{"m":65536,"t":3,"p":4}',
    salt            BLOB NOT NULL,
    verifier        BLOB NOT NULL,
    encrypted_dek   BLOB NOT NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS security_state (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version  INTEGER NOT NULL DEFAULT 1,   -- Security Metadata 自身版本
    unlock_strategy INTEGER NOT NULL DEFAULT 1,   -- 0=Automatic 1=Manual 2=Session
    migrated        INTEGER NOT NULL DEFAULT 0,   -- 旧 keyring 凭据是否已双写
    verified        INTEGER NOT NULL DEFAULT 0,   -- 迁移后验证通过
    last_unlock     TEXT,
    failed_attempts INTEGER NOT NULL DEFAULT 0,
    last_rotation   TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS security_credentials (
    id            TEXT PRIMARY KEY,
    kind          INTEGER NOT NULL,               -- SecretKind.to_db()
    name          TEXT,
    version       INTEGER NOT NULL DEFAULT 1,     -- 算法/编码演进版本
    algorithm_id  INTEGER NOT NULL DEFAULT 1,     -- AlgorithmId
    nonce         BLOB NOT NULL,
    ciphertext    BLOB NOT NULL,
    metadata      TEXT NOT NULL DEFAULT '{}',      -- serde_json::Value
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_security_credentials_kind ON security_credentials(kind);
"#;
