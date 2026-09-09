//! Project Export & Import services.
//!
//! Provides single-project export (.vproj JSON or AES-GCM encrypted envelope)
//! with configurable sub-modules (sessions, AI records, tunnels, service monitors,
//! snippets, quick commands, session history) and safe import with duplicate resolution strategies.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use velowork_core::storage::{Database, database};
use velowork_core::theme::FolderColor;
use velowork_state::{ProjectData, ServiceDefinition, TunnelProfile};

use crate::repositories::ai::{AiConversationRow, AiMessageRow, AiRepository};
use crate::repositories::history::{HistoryEntry, HistoryRepository};
use crate::repositories::service_tree::ServiceTreeRepository;
use crate::repositories::snippet::SnippetRepository;
use crate::repositories::ssh_session_tree::{SessionTreeRow, SshSessionTreeRepository};
use crate::repositories::tunnel_tree::TunnelTreeRepository;
use crate::quick_commands::QuickCommandNode;

pub const VPROJ_MAGIC: &str = "VELOWORK_VPROJ";
pub const VPROJ_ENCRYPTED_MAGIC: &str = "VELOWORK_VPROJ_ENCRYPTED";
pub const VPROJ_SCHEMA_VERSION: u32 = 1;
const PBKDF2_ROUNDS: u32 = 100_000;

/// Options configuring which modules to include in a project export.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectExportOptions {
    /// Sessions/terminals (always true).
    pub include_sessions: bool,
    /// AI conversation history.
    pub include_ai_records: bool,
    /// Associated tunnels.
    pub include_tunnels: bool,
    /// Service monitoring definitions.
    pub include_services: bool,
    /// Quick commands / snippets.
    pub include_snippets: bool,
    /// Session access / command history.
    pub include_session_history: bool,
}

impl Default for ProjectExportOptions {
    fn default() -> Self {
        Self {
            include_sessions: true,
            include_ai_records: true,
            include_tunnels: true,
            include_services: true,
            include_snippets: true,
            include_session_history: true,
        }
    }
}

/// A serialized snippet export entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnippetExportItem {
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// A serialized access history entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionHistoryExportItem {
    pub session_id: String,
    pub accessed_at_ms: u64,
}

/// A serialized AI conversation export item (including its messages).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiConversationExportItem {
    pub conversation: AiConversationRow,
    #[serde(default)]
    pub messages: Vec<AiMessageRow>,
}

/// Plaintext Project Export Package.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectExportPackage {
    pub magic: String,
    pub schema_version: u32,
    pub exported_at: u64,
    pub project: ProjectData,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Vec<SessionTreeRow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_conversations: Option<Vec<AiConversationExportItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_records: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnels: Option<Vec<TunnelProfile>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services: Option<Vec<ServiceDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippets: Option<Vec<SnippetExportItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_commands: Option<Vec<QuickCommandNode>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_history: Option<Vec<HistoryEntry>>,
}

/// Encrypted envelope when export password is used.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedProjectEnvelope {
    pub magic: String,
    pub version: u32,
    pub salt: String,       // base64
    pub nonce: String,      // base64
    pub ciphertext: String, // base64
}

/// Parsed summary returned to the UI after analyzing an export file.
#[derive(Clone, Debug)]
pub struct ParsedProjectSummary {
    pub project_name: String,
    pub project_desc: String,
    pub icon: String,
    pub color: FolderColor,
    pub session_count: usize,
    pub ai_conversation_count: usize,
    pub has_ai_records: bool,
    pub tunnel_count: usize,
    pub service_count: usize,
    pub snippet_count: usize,
    pub quick_command_count: usize,
    pub history_count: usize,
    pub is_encrypted: bool,
    pub package: ProjectExportPackage,
}

/// Duplicate project name resolution policy on import.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ProjectDuplicateStrategy {
    #[default]
    Rename,
    Overwrite,
    Skip,
}

impl ProjectDuplicateStrategy {
    pub fn all() -> &'static [ProjectDuplicateStrategy] {
        &[
            ProjectDuplicateStrategy::Rename,
            ProjectDuplicateStrategy::Overwrite,
            ProjectDuplicateStrategy::Skip,
        ]
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Export Implementation
// ─────────────────────────────────────────────────────────────────────────────

pub struct ProjectExportService;

impl ProjectExportService {
    /// Build a `ProjectExportPackage` for `project_id` using the current active profile.
    pub fn build_package(
        project: &ProjectData,
        options: &ProjectExportOptions,
        db: Option<Arc<Database>>,
        quick_commands: Option<&[QuickCommandNode]>,
    ) -> Result<ProjectExportPackage> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let active_db = db.or_else(database);

        // 1. Sessions: SSH session tree nodes associated with this project
        let sessions = if options.include_sessions {
            if let Some(ref db) = active_db {
                let repo = SshSessionTreeRepository::new(db.clone());
                let rows = repo.export_rows().unwrap_or_default();
                let filtered: Vec<SessionTreeRow> = rows
                    .into_iter()
                    .filter(|r| r.project_id.as_deref() == Some(&project.id))
                    .collect();
                Some(filtered)
            } else {
                Some(Vec::new())
            }
        } else {
            None
        };

        // 2. AI records: load all conversations and messages from AiRepository
        let (ai_conversations, ai_records) = if options.include_ai_records {
            if let Some(ref db) = active_db {
                let repo = AiRepository::new(db.clone());
                let convs = repo.list_conversations(Some(&project.id)).unwrap_or_default();
                let mut items = Vec::new();
                for conv in convs {
                    let msgs = repo.list_messages(&conv.id).unwrap_or_default();
                    items.push(AiConversationExportItem {
                        conversation: conv,
                        messages: msgs,
                    });
                }
                let first_msgs = items.first().and_then(|item| serde_json::to_value(&item.messages).ok());
                (Some(items), first_msgs)
            } else {
                (Some(Vec::new()), None)
            }
        } else {
            (None, None)
        };

        // 3. Tunnels: profiles where project_id matches
        let tunnels = if options.include_tunnels {
            let nodes = if let Some(ref db) = active_db {
                TunnelTreeRepository::new(db.clone()).load_tree().unwrap_or_default()
            } else {
                Vec::new()
            };
            let mut list = Vec::new();
            collect_tunnels_for_project(&nodes, Some(&project.id), &mut list);
            Some(list)
        } else {
            None
        };

        // 4. Services: service definitions where project_id matches
        let services = if options.include_services {
            let nodes = if let Some(ref db) = active_db {
                ServiceTreeRepository::new(db.clone()).load_tree().unwrap_or_default()
            } else {
                Vec::new()
            };
            let mut defs = Vec::new();
            collect_services_for_project(&nodes, Some(&project.id), &mut defs);
            Some(defs)
        } else {
            None
        };

        // 5. Quick Commands & Snippets
        let quick_commands = if options.include_snippets {
            quick_commands.map(|qc| qc.to_vec())
        } else {
            None
        };

        let snippets = if options.include_snippets {
            if let Some(ref db) = active_db {
                let repo = SnippetRepository::new(db.clone());
                if let Ok(all) = repo.list() {
                    let items = all
                        .into_iter()
                        .map(|s| SnippetExportItem {
                            id: s.id,
                            name: s.name,
                            content: s.content,
                            language: s.language,
                            tags: s.tags,
                        })
                        .collect();
                    Some(items)
                } else {
                    Some(Vec::new())
                }
            } else {
                Some(Vec::new())
            }
        } else {
            None
        };

        // 6. Session / Command History
        let session_history = if options.include_session_history {
            if let Some(ref db) = active_db {
                let repo = HistoryRepository::new(db.clone());
                repo.list_by_project(&project.id, None, 5000).ok()
            } else {
                Some(Vec::new())
            }
        } else {
            None
        };

        Ok(ProjectExportPackage {
            magic: VPROJ_MAGIC.to_string(),
            schema_version: VPROJ_SCHEMA_VERSION,
            exported_at: now_ms,
            project: project.clone(),
            sessions,
            ai_conversations,
            ai_records,
            tunnels,
            services,
            snippets,
            quick_commands,
            session_history,
        })
    }

    /// Export a project to bytes (either pretty JSON or encrypted envelope if password is provided).
    pub fn export_to_bytes(
        project: &ProjectData,
        options: &ProjectExportOptions,
        password: Option<&str>,
        db: Option<Arc<Database>>,
        quick_commands: Option<&[QuickCommandNode]>,
    ) -> Result<Vec<u8>> {
        let package = Self::build_package(project, options, db, quick_commands)?;
        let json_bytes = serde_json::to_vec_pretty(&package).context("serialize project package")?;

        if let Some(pwd) = password {
            let pwd = pwd.trim();
            if !pwd.is_empty() {
                return Self::encrypt_payload(&json_bytes, pwd);
            }
        }

        Ok(json_bytes)
    }

    fn encrypt_payload(plaintext: &[u8], password: &str) -> Result<Vec<u8>> {
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);

        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, PBKDF2_ROUNDS, &mut key);

        let cipher = Aes256Gcm::new_from_slice(&key).context("create aes256gcm cipher")?;

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("aes-gcm encryption failed: {:?}", e))?;

        let b64 = base64::engine::general_purpose::STANDARD;
        let envelope = EncryptedProjectEnvelope {
            magic: VPROJ_ENCRYPTED_MAGIC.to_string(),
            version: VPROJ_SCHEMA_VERSION,
            salt: b64.encode(salt),
            nonce: b64.encode(nonce_bytes),
            ciphertext: b64.encode(ciphertext),
        };

        serde_json::to_vec_pretty(&envelope).context("serialize encrypted envelope")
    }

    /// Parse project export bytes (supports plaintext and password-protected encrypted files).
    pub fn parse_package(
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<ParsedProjectSummary> {
        let b64 = base64::engine::general_purpose::STANDARD;

        // 1. Try parsing as encrypted envelope first
        if let Ok(envelope) = serde_json::from_slice::<EncryptedProjectEnvelope>(bytes) {
            if envelope.magic == VPROJ_ENCRYPTED_MAGIC {
                let pwd = match password {
                    Some(p) if !p.trim().is_empty() => p.trim(),
                    _ => {
                        bail!("PASSWORD_REQUIRED");
                    }
                };

                let salt = b64.decode(&envelope.salt).context("invalid salt base64")?;
                let nonce_bytes = b64.decode(&envelope.nonce).context("invalid nonce base64")?;
                let ciphertext = b64.decode(&envelope.ciphertext).context("invalid ciphertext base64")?;

                if nonce_bytes.len() != 12 {
                    bail!("invalid nonce length");
                }

                let mut key = [0u8; 32];
                pbkdf2_hmac::<Sha256>(pwd.as_bytes(), &salt, PBKDF2_ROUNDS, &mut key);

                let cipher = Aes256Gcm::new_from_slice(&key).context("create aes256gcm cipher")?;
                let nonce = Nonce::from_slice(&nonce_bytes);

                let decrypted = cipher
                    .decrypt(nonce, ciphertext.as_ref())
                    .map_err(|_| anyhow::anyhow!("PASSWORD_INCORRECT"))?;

                let pkg: ProjectExportPackage = serde_json::from_slice(&decrypted)
                    .context("invalid decrypted project package format")?;

                return Ok(Self::package_to_summary(pkg, true));
            }
        }

        // 2. Try parsing as plaintext ProjectExportPackage
        let pkg: ProjectExportPackage = serde_json::from_slice(bytes)
            .context("invalid project package format")?;

        Ok(Self::package_to_summary(pkg, false))
    }

    fn package_to_summary(package: ProjectExportPackage, is_encrypted: bool) -> ParsedProjectSummary {
        let project_name = package.project.name.clone();
        let project_desc = package.project.description.clone();
        let icon = package.project.icon.clone();
        let color = package.project.folder_color;
        let session_count = package.sessions.as_ref().map(|s| s.len()).unwrap_or(0);
        let ai_conversation_count = package.ai_conversations.as_ref().map(|c| c.len()).unwrap_or_else(|| {
            if package.ai_records.as_ref().map_or(false, |v| !v.is_null()) { 1 } else { 0 }
        });
        let has_ai_records = ai_conversation_count > 0;
        let tunnel_count = package.tunnels.as_ref().map(|t| t.len()).unwrap_or(0);
        let service_count = package.services.as_ref().map(|s| s.len()).unwrap_or(0);
        let snippet_count = package.snippets.as_ref().map(|s| s.len()).unwrap_or(0);
        let quick_command_count = package.quick_commands.as_ref().map(|q| q.len()).unwrap_or(0);
        let history_count = package.session_history.as_ref().map(|h| h.len()).unwrap_or(0);

        ParsedProjectSummary {
            project_name,
            project_desc,
            icon,
            color,
            session_count,
            ai_conversation_count,
            has_ai_records,
            tunnel_count,
            service_count,
            snippet_count,
            quick_command_count,
            history_count,
            is_encrypted,
            package,
        }
    }
}

fn collect_tunnels_for_project(
    nodes: &[velowork_state::TunnelNode],
    project_id: Option<&str>,
    out: &mut Vec<TunnelProfile>,
) {
    for node in nodes {
        match node {
            velowork_state::TunnelNode::Tunnel { profile } => {
                if profile.project_id.as_deref() == project_id {
                    out.push(profile.clone());
                }
            }
            velowork_state::TunnelNode::Folder { children, .. } => {
                collect_tunnels_for_project(children, project_id, out);
            }
        }
    }
}

fn collect_services_for_project(
    nodes: &[velowork_state::ServiceNode],
    project_id: Option<&str>,
    out: &mut Vec<ServiceDefinition>,
) {
    for node in nodes {
        match node {
            velowork_state::ServiceNode::Service { def } => {
                if def.project_id.as_deref() == project_id {
                    out.push(def.clone());
                }
            }
            velowork_state::ServiceNode::Folder { children, .. } => {
                collect_services_for_project(children, project_id, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_and_parse_plaintext() {
        let mut proj = ProjectData::default();
        proj.id = "proj-1".to_string();
        proj.name = "My Test Project".to_string();
        proj.description = "Test description".to_string();

        let options = ProjectExportOptions::default();
        let bytes = ProjectExportService::export_to_bytes(&proj, &options, None, None, None).unwrap();
        let summary = ProjectExportService::parse_package(&bytes, None).unwrap();

        assert_eq!(summary.project_name, "My Test Project");
        assert_eq!(summary.project_desc, "Test description");
        assert!(!summary.is_encrypted);
    }

    #[test]
    fn test_export_and_parse_encrypted() {
        let mut proj = ProjectData::default();
        proj.id = "proj-2".to_string();
        proj.name = "Secret Project".to_string();

        let options = ProjectExportOptions::default();
        let bytes = ProjectExportService::export_to_bytes(&proj, &options, Some("secret_pass123"), None, None).unwrap();

        // Parse without password -> should fail with PASSWORD_REQUIRED
        let err = ProjectExportService::parse_package(&bytes, None);
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("PASSWORD_REQUIRED"));

        // Parse with wrong password -> should fail with PASSWORD_INCORRECT
        let err = ProjectExportService::parse_package(&bytes, Some("wrong_pass"));
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("PASSWORD_INCORRECT"));

        // Parse with correct password -> should succeed
        let summary = ProjectExportService::parse_package(&bytes, Some("secret_pass123")).unwrap();
        assert_eq!(summary.project_name, "Secret Project");
        assert!(summary.is_encrypted);
    }

    #[test]
    fn test_export_options_module_filtering() {
        let mut proj = ProjectData::default();
        proj.id = "proj-3".to_string();
        proj.name = "Filtered Project".to_string();

        let mut options = ProjectExportOptions::default();
        options.include_ai_records = false;
        options.include_tunnels = false;
        options.include_services = false;

        let pkg = ProjectExportService::build_package(&proj, &options, None, None).unwrap();
        assert!(pkg.sessions.is_some());
        assert!(pkg.ai_records.is_none());
        assert!(pkg.ai_conversations.is_none());
        assert!(pkg.tunnels.is_none());
        assert!(pkg.services.is_none());
    }
}
