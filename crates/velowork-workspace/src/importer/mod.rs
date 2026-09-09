//! Unified session importer framework.
//!
//! Provides an extensible abstraction for importing SSH/terminal sessions from
//! various third-party formats (Xshell, MobaXterm, WindTerm, FinalShell, etc.)
//! into Velowork's hierarchical session tree.

pub mod common;
pub mod finalshell;
pub mod merge;
pub mod mobaxterm;
pub mod registry;
pub mod windterm;
pub mod xshell;

#[cfg(test)]
mod tests;

use anyhow::Result;
use std::path::{Path, PathBuf};
use velowork_state::{SessionProtocol, SshAuthType};

pub use merge::{DuplicateStrategy, import_sessions_into_store};
pub use registry::ImporterRegistry;

/// Standardized intermediate session representation parsed from external formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedSession {
    /// Display name of the session.
    pub name: String,
    /// Protocol type (defaults to SSH).
    pub protocol: SessionProtocol,
    /// Target host or IP address.
    pub host: String,
    /// Connection port (default 22 for SSH).
    pub port: u16,
    /// Username (default "root").
    pub username: String,
    /// Authentication configuration (Password / PrivateKey / Agent / KeyboardInteractive).
    pub auth_type: SshAuthType,
    /// Folder hierarchy segments, e.g. `Some(vec!["Prod", "Web"])`.
    pub group_path: Option<Vec<String>>,
    /// Optional description / remark.
    pub description: Option<String>,
}

impl Default for ImportedSession {
    fn default() -> Self {
        Self {
            name: String::new(),
            protocol: SessionProtocol::Ssh,
            host: String::new(),
            port: 22,
            username: "root".to_string(),
            auth_type: SshAuthType::Password { password: None },
            group_path: None,
            description: None,
        }
    }
}

/// Execution context passed to importers during parse.
#[derive(Clone, Debug)]
pub struct ImportContext {
    /// Absolute or relative path to the import file or directory.
    pub source_path: PathBuf,
    /// Optional master password for formats supporting encrypted credentials (e.g. WindTerm).
    pub master_password: Option<String>,
    /// Target project ID to import into (`None` for active/default project).
    pub target_project_id: Option<String>,
}

impl ImportContext {
    pub fn new(source_path: impl Into<PathBuf>) -> Self {
        Self {
            source_path: source_path.into(),
            master_password: None,
            target_project_id: None,
        }
    }

    pub fn with_master_password(mut self, password: impl Into<String>) -> Self {
        self.master_password = Some(password.into());
        self
    }

    pub fn with_target_project(mut self, project_id: impl Into<String>) -> Self {
        self.target_project_id = Some(project_id.into());
        self
    }
}

/// Summary report returned after importing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportResult {
    /// Number of sessions found in the source file/directory.
    pub total_found: usize,
    /// Number of sessions successfully inserted or updated in the tree.
    pub imported_sessions: usize,
    /// Number of sessions renamed due to sibling name collision.
    pub renamed_sessions: usize,
    /// Number of existing sessions overwritten.
    pub overwritten_sessions: usize,
    /// Number of sessions skipped due to duplicate name.
    pub skipped_sessions: usize,
    /// Number of new folders created during hierarchy construction.
    pub created_folders: usize,
    /// Non-fatal warning messages.
    pub warnings: Vec<String>,
}

/// Extensible trait implemented by all session format importers.
pub trait SessionImporter: Send + Sync {
    /// Unique identifier for this importer, e.g. "xshell", "mobaxterm", "windterm", "finalshell".
    fn id(&self) -> &'static str;

    /// User-facing display name, e.g. "Xshell (.xts, .xsh)".
    fn display_name(&self) -> &'static str;

    /// File extensions supported by this importer (with leading dot), e.g. `&[".xts", ".xsh"]`.
    fn file_extensions(&self) -> &'static [&'static str];

    /// Whether this importer targets a directory instead of a single file (e.g. FinalShell conn dir).
    fn is_directory_importer(&self) -> bool {
        false
    }

    /// Fast check whether this importer can handle the specified file or directory path.
    fn can_handle(&self, path: &Path) -> bool {
        if self.is_directory_importer() {
            path.is_dir()
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let dot_ext = format!(".{}", ext.to_lowercase());
            self.file_extensions()
                .iter()
                .any(|e| e.eq_ignore_ascii_case(&dot_ext))
        } else {
            false
        }
    }

    /// Parse external format into normalized `ImportedSession` list.
    fn parse(&self, ctx: &ImportContext) -> Result<Vec<ImportedSession>>;
}
