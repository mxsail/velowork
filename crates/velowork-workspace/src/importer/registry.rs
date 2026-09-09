use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use super::finalshell::FinalShellImporter;
use super::mobaxterm::MobaXtermImporter;
use super::windterm::WindTermImporter;
use super::xshell::XshellImporter;
use super::{ImportContext, ImportedSession, SessionImporter};

/// Registry of session format importers.
#[derive(Clone, Default)]
pub struct ImporterRegistry {
    importers: Vec<Arc<dyn SessionImporter>>,
}

impl ImporterRegistry {
    pub fn new() -> Self {
        Self {
            importers: Vec::new(),
        }
    }

    /// Creates a registry pre-populated with default built-in importers
    /// (Xshell, MobaXterm, WindTerm, FinalShell).
    pub fn default_registry() -> Self {
        let mut registry = Self::new();
        registry.register(XshellImporter);
        registry.register(MobaXtermImporter);
        registry.register(WindTermImporter);
        registry.register(FinalShellImporter);
        registry
    }

    /// Register a new format importer into the registry.
    pub fn register<T: SessionImporter + 'static>(&mut self, importer: T) {
        self.importers.push(Arc::new(importer));
    }

    /// List all registered importers.
    pub fn all_importers(&self) -> &[Arc<dyn SessionImporter>] {
        &self.importers
    }

    /// Find an importer by its unique string ID (e.g. "xshell").
    pub fn find_by_id(&self, id: &str) -> Option<Arc<dyn SessionImporter>> {
        self.importers
            .iter()
            .find(|imp| imp.id().eq_ignore_ascii_case(id))
            .cloned()
    }

    /// Auto-detect and return an importer capable of handling the target path.
    pub fn find_for_path(&self, path: &Path) -> Option<Arc<dyn SessionImporter>> {
        self.importers
            .iter()
            .find(|imp| imp.can_handle(path))
            .cloned()
    }

    /// Execute parse using either a specified format ID or auto-detection from file path.
    pub fn parse(
        &self,
        format_id: Option<&str>,
        ctx: &ImportContext,
    ) -> Result<Vec<ImportedSession>> {
        let importer = if let Some(id) = format_id {
            if id == "auto" || id.is_empty() {
                self.find_for_path(&ctx.source_path).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Could not auto-detect format for path: {}. Please select a specific format.",
                        ctx.source_path.display()
                    )
                })?
            } else {
                self.find_by_id(id)
                    .ok_or_else(|| anyhow::anyhow!("Unknown importer format ID: {id}"))?
            }
        } else {
            self.find_for_path(&ctx.source_path).ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not auto-detect format for path: {}. Please select a specific format.",
                    ctx.source_path.display()
                )
            })?
        };

        importer
            .parse(ctx)
            .with_context(|| format!("Failed to parse sessions using {}", importer.display_name()))
    }
}
