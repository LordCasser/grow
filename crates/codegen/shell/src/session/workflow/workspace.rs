use std::path::Path;

use serde::{Deserialize, Serialize};
use tools::implementations::grow_build::workflow::{
    WorkflowDefinitionId, WorkflowDefinitionSummary, WorkflowDiagnostic, WorkflowDraftSource,
    WorkflowScope,
};

use super::registry::{self, ResolveError, ResolvedWorkflow, WorkflowRegistry, WorkflowSource};

const WORKSPACE_VERSION: u8 = 1;
const MAX_WORKSPACE_STATE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub(crate) enum WorkspaceError {
    #[error("workflow workspace I/O failed at {path}: {error}")]
    Io { path: String, error: String },
    #[error("workflow workspace state is invalid: {0}")]
    InvalidState(String),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error("unknown Workflow Definition: {0}")]
    UnknownDefinition(String),
    #[error("workflow draft already exists for '{0}'")]
    DraftAlreadyExists(String),
    #[error("Workflow Definition is not an editable session draft: {0}")]
    NotDraft(String),
    #[error("workflow draft must be validated at its current content hash before publishing")]
    NotValidated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DraftSource {
    Inline,
    File {
        path: String,
    },
    Definition {
        definition_id: WorkflowDefinitionId,
        scope: WorkflowScope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        baseline_hash: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DraftRecord {
    definition_id: WorkflowDefinitionId,
    name: String,
    script_file: String,
    content_hash: String,
    source: DraftSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_validated_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    save_prompted_hash: Option<String>,
    #[serde(default)]
    conflicted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingPublish {
    draft_definition_id: WorkflowDefinitionId,
    target_definition_id: WorkflowDefinitionId,
    scope: WorkflowScope,
    target_path: String,
    content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_base_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceState {
    version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    focused_definition_id: Option<WorkflowDefinitionId>,
    #[serde(default)]
    drafts: Vec<DraftRecord>,
    #[serde(default)]
    validated_hashes: std::collections::HashMap<WorkflowDefinitionId, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_publish: Option<PendingPublish>,
}

impl Default for WorkspaceState {
    fn default() -> Self {
        Self {
            version: WORKSPACE_VERSION,
            focused_definition_id: None,
            drafts: Vec::new(),
            validated_hashes: std::collections::HashMap::new(),
            pending_publish: None,
        }
    }
}

pub(crate) struct WorkflowDefinition {
    pub summary: WorkflowDefinitionSummary,
    pub resolved: ResolvedWorkflow,
}

pub(crate) struct WorkflowCatalog {
    pub definitions: Vec<WorkflowDefinitionSummary>,
    pub diagnostics: Vec<WorkflowDiagnostic>,
}

pub(crate) struct WorkflowWorkspace {
    root: crate::session::storage::ContainedDirectory,
    state: WorkspaceState,
}

impl WorkflowWorkspace {
    #[cfg(test)]
    pub(crate) fn open(session_dir: &Path, cwd: &Path) -> Result<Self, WorkspaceError> {
        let session = crate::session::storage::ContainedDirectory::open(
            session_dir,
            Path::new(""),
            "Workflow test session",
            false,
        )
        .map_err(|error| WorkspaceError::Io {
            path: session_dir.display().to_string(),
            error: error.to_string(),
        })?;
        Self::open_in_session(&session, cwd)
    }

    pub(crate) fn open_in_session(
        session: &crate::session::storage::ContainedDirectory,
        cwd: &Path,
    ) -> Result<Self, WorkspaceError> {
        let root = session
            .open_relative(
                Path::new("workflow-workspace"),
                "Workflow workspace",
                true,
            )
            .map_err(|error| WorkspaceError::Io {
                path: session
                    .display_path()
                    .join("workflow-workspace")
                    .display()
                    .to_string(),
                error: error.to_string(),
            })?;
        let state = load_state(&root)?.unwrap_or_default();
        let mut workspace = Self { root, state };
        workspace.recover_pending_publish(cwd)?;
        workspace.refresh_content_hashes()?;
        Ok(workspace)
    }

    pub(crate) fn catalog(&self, cwd: &Path) -> WorkflowCatalog {
        let registry = WorkflowRegistry::scan(Some(cwd));
        let mut definitions = Vec::new();
        let mut diagnostics = registry.diagnostics().to_vec();
        for draft in &self.state.drafts {
            match self.definition_from_draft(draft) {
                Ok(definition) => {
                    if definition.summary.status.contains("conflicted") {
                        diagnostics.push(WorkflowDiagnostic {
                            scope: WorkflowScope::Session,
                            path: definition.summary.path.clone(),
                            code: "publish_conflict".into(),
                            message: format!(
                                "draft '{}' conflicts with an externally modified saved Definition",
                                definition.summary.name
                            ),
                        });
                    }
                    definitions.push(definition.summary);
                }
                Err(error) => diagnostics.push(WorkflowDiagnostic {
                    scope: WorkflowScope::Session,
                    path: Some(self.draft_display_path(draft).display().to_string()),
                    code: "invalid_draft".into(),
                    message: error.to_string(),
                }),
            }
        }
        definitions.extend(registry.list().into_iter().map(|listing| {
            let validated = self
                .state
                .validated_hashes
                .get(&listing.definition_id)
                .is_some_and(|validated_hash| validated_hash == &listing.content_hash);
            WorkflowDefinitionSummary {
                focused: self
                    .state
                    .focused_definition_id
                    .as_ref()
                    .is_some_and(|focused| focused == &listing.definition_id),
                definition_id: listing.definition_id,
                name: listing.name,
                description: listing.description,
                when_to_use: listing.when_to_use,
                scope: listing.scope,
                status: if validated {
                    "saved,validated".into()
                } else {
                    "saved".into()
                },
                path: listing.path,
                draft_source: None,
                source_definition_id: None,
                source_path: None,
                content_hash: listing.content_hash,
            }
        }));
        WorkflowCatalog {
            definitions,
            diagnostics,
        }
    }

    pub(crate) fn compact_context(&self, cwd: &Path) -> String {
        let catalog = self.catalog(cwd);
        let focus = catalog
            .definitions
            .iter()
            .find(|definition| definition.focused)
            .map(|definition| {
                format!(
                    "{} [{}] ({}, {})",
                    definition.name,
                    definition.definition_id,
                    definition.scope.as_str(),
                    definition.status
                )
            })
            .unwrap_or_else(|| "none".into());
        let drafts = catalog
            .definitions
            .iter()
            .filter(|definition| definition.scope == WorkflowScope::Session)
            .count();
        format!(
            "Workflow workspace: focus={focus}; definitions={}; drafts={drafts}; diagnostics={}. Source is loaded only through inspect.",
            catalog.definitions.len(),
            catalog.diagnostics.len()
        )
    }

    pub(crate) fn command_listings(&self, cwd: &Path) -> Vec<super::registry::WorkflowListing> {
        self.catalog(cwd)
            .definitions
            .into_iter()
            .map(|definition| super::registry::WorkflowListing {
                definition_id: definition.definition_id,
                name: definition.name,
                description: definition.description,
                when_to_use: definition.when_to_use,
                source: definition.scope.as_str(),
                scope: definition.scope,
                path: definition.path,
                status: definition.status,
                content_hash: definition.content_hash,
                focused: definition.focused,
            })
            .collect()
    }

    pub(crate) fn search(&self, cwd: &Path, query: &str, limit: usize) -> WorkflowCatalog {
        let query = query.trim().to_lowercase();
        let mut catalog = self.catalog(cwd);
        catalog.definitions.retain(|definition| {
            [
                definition.name.as_str(),
                definition.description.as_str(),
                definition.when_to_use.as_deref().unwrap_or(""),
            ]
            .iter()
            .any(|field| field.to_lowercase().contains(&query))
        });
        catalog.definitions.sort_by_key(|definition| {
            let exact = definition.name == query;
            let focused = definition.focused;
            let scope = match definition.scope {
                WorkflowScope::Session => 0,
                WorkflowScope::Project => 1,
                WorkflowScope::User => 2,
                WorkflowScope::Builtin => 3,
            };
            (!exact, !focused, scope, definition.name.clone())
        });
        catalog.definitions.truncate(limit);
        catalog
    }

    pub(crate) fn resolve(
        &self,
        cwd: &Path,
        definition_id: &WorkflowDefinitionId,
    ) -> Result<WorkflowDefinition, WorkspaceError> {
        if let Some(draft) = self
            .state
            .drafts
            .iter()
            .find(|draft| &draft.definition_id == definition_id)
        {
            return self.definition_from_draft(draft);
        }
        let resolved = WorkflowRegistry::scan(Some(cwd)).resolve_by_id(definition_id)?;
        Ok(self.definition_from_saved(resolved))
    }

    pub(crate) fn focus(
        &mut self,
        cwd: &Path,
        definition_id: &WorkflowDefinitionId,
    ) -> Result<(), WorkspaceError> {
        let _lock = self.lock_state()?;
        self.reload_state()?;
        self.resolve(cwd, definition_id)?;
        self.state.focused_definition_id = Some(definition_id.clone());
        self.persist()
    }

    pub(crate) fn draft(
        &mut self,
        cwd: &Path,
        expected_name: Option<&str>,
        input: WorkflowDraftSource,
    ) -> Result<WorkflowDefinition, WorkspaceError> {
        let _lock = self.lock_state()?;
        self.reload_state()?;
        let (resolved, source) = match input {
            WorkflowDraftSource::Definition { definition_id } => {
                let source = self.resolve(cwd, &definition_id)?;
                if source.summary.scope == WorkflowScope::Session {
                    return Err(WorkspaceError::DraftAlreadyExists(source.summary.name));
                }
                let draft_source = DraftSource::Definition {
                    definition_id: source.summary.definition_id.clone(),
                    scope: source.summary.scope,
                    path: source.summary.path.clone(),
                    baseline_hash: source.summary.content_hash.clone(),
                };
                (source.resolved, draft_source)
            }
            WorkflowDraftSource::Inline { script } => {
                (registry::resolve_inline(script)?, DraftSource::Inline)
            }
            WorkflowDraftSource::File { path } => {
                let resolved = registry::resolve_by_path(Path::new(&path), cwd, None)?;
                let source_path = match &resolved.source {
                    WorkflowSource::File(path) => path.display().to_string(),
                    WorkflowSource::Builtin | WorkflowSource::Inline => {
                        return Err(WorkspaceError::InvalidState(
                            "trusted draft file did not resolve to a file source".into(),
                        ));
                    }
                };
                (resolved, DraftSource::File { path: source_path })
            }
        };
        if let Some(expected_name) = expected_name
            && expected_name != resolved.meta.name
        {
            return Err(WorkspaceError::InvalidState(format!(
                "requested draft name '{expected_name}' does not match meta.name '{}'",
                resolved.meta.name
            )));
        }
        if self
            .state
            .drafts
            .iter()
            .any(|draft| draft.name == resolved.meta.name)
        {
            return Err(WorkspaceError::DraftAlreadyExists(resolved.meta.name));
        }

        let uuid = uuid::Uuid::now_v7().simple().to_string();
        let definition_id = WorkflowDefinitionId::new(format!("session:{uuid}"));
        let script_file = format!("drafts/{uuid}.rhai");
        let record = DraftRecord {
            definition_id: definition_id.clone(),
            name: resolved.meta.name,
            script_file,
            content_hash: resolved.content_hash.clone(),
            source,
            last_validated_hash: None,
            save_prompted_hash: None,
            conflicted: false,
        };
        self.write_draft(&record, &resolved.script)?;
        self.state.drafts.push(record);
        self.state.focused_definition_id = Some(definition_id.clone());
        self.persist()?;
        self.resolve(cwd, &definition_id)
    }

    pub(crate) fn record_validated(
        &mut self,
        cwd: &Path,
        definition_id: &WorkflowDefinitionId,
        hash: &str,
    ) -> Result<(), WorkspaceError> {
        let _lock = self.lock_state()?;
        self.reload_state()?;
        let definition = self.resolve(cwd, definition_id)?;
        if definition.summary.content_hash != hash {
            return Err(WorkspaceError::InvalidState(
                "Definition changed while validation was running".into(),
            ));
        }
        if let Some(draft) = self
            .state
            .drafts
            .iter_mut()
            .find(|draft| &draft.definition_id == definition_id)
        {
            draft.content_hash = hash.to_string();
            draft.last_validated_hash = Some(hash.to_string());
        } else {
            self.state
                .validated_hashes
                .insert(definition_id.clone(), hash.to_string());
        }
        self.state.focused_definition_id = Some(definition_id.clone());
        self.persist()
    }

    pub(crate) fn publish(
        &mut self,
        cwd: &Path,
        definition_id: &WorkflowDefinitionId,
        scope: WorkflowScope,
    ) -> Result<WorkflowDefinition, WorkspaceError> {
        let _lock = self.lock_state()?;
        self.reload_state()?;
        let index = self
            .state
            .drafts
            .iter()
            .position(|draft| &draft.definition_id == definition_id)
            .ok_or_else(|| WorkspaceError::NotDraft(definition_id.0.clone()))?;
        let record = self.state.drafts[index].clone();
        let draft = self.definition_from_draft(&record)?;
        if record.last_validated_hash.as_deref() != Some(&draft.summary.content_hash) {
            return Err(WorkspaceError::NotValidated);
        }
        let target_id = registry::definition_id(scope, &draft.summary.name);
        let expected_base_hash = match &record.source {
            DraftSource::Definition {
                definition_id,
                baseline_hash,
                ..
            } if definition_id == &target_id => Some(baseline_hash.as_str()),
            DraftSource::Inline | DraftSource::File { .. } | DraftSource::Definition { .. } => None,
        };
        let target_path = registry::publish_target_path(cwd, scope, &draft.summary.name)?;
        self.state.pending_publish = Some(PendingPublish {
            draft_definition_id: definition_id.clone(),
            target_definition_id: target_id.clone(),
            scope,
            target_path: target_path.display().to_string(),
            content_hash: draft.summary.content_hash.clone(),
            expected_base_hash: expected_base_hash.map(str::to_owned),
        });
        self.persist()?;
        let path = match registry::publish_workflow(
            cwd,
            scope,
            &draft.summary.name,
            &draft.resolved.script,
            expected_base_hash,
        ) {
            Ok(path) => path,
            Err(error) => {
                // An error normally means the atomic target commit did not
                // happen. Re-observe filesystem truth because a platform I/O
                // error may still arrive after the rename became durable.
                if self.resolve_pending_publish(cwd)? {
                    let mut resolved = registry::resolve_by_path(&target_path, cwd, None)?;
                    resolved.definition_id = target_id;
                    resolved.scope = scope;
                    return Ok(self.definition_from_saved(resolved));
                }
                return Err(error.into());
            }
        };
        let draft_path = self.complete_pending_publish()?;
        self.persist()?;
        if let Some(script_file) = draft_path
            && let Err(error) = self.remove_draft_file(&script_file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.root.display_path().join(&script_file).display(), %error, "failed to remove published workflow draft");
        }
        let mut resolved = registry::resolve_by_path(&path, cwd, None)?;
        resolved.definition_id = target_id;
        resolved.scope = scope;
        Ok(self.definition_from_saved(resolved))
    }

    /// Return `true` exactly once for each successfully completed content hash
    /// of a session draft and persist the acknowledgement before surfacing it.
    pub(crate) fn take_save_prompt(
        &mut self,
        definition_id: &WorkflowDefinitionId,
        content_hash: &str,
    ) -> Result<bool, WorkspaceError> {
        let _lock = self.lock_state()?;
        self.reload_state()?;
        let Some(draft) = self
            .state
            .drafts
            .iter()
            .find(|draft| &draft.definition_id == definition_id)
        else {
            return Ok(false);
        };
        let script = self.read_draft(draft)?;
        if registry::content_hash(&script) != content_hash {
            return Ok(false);
        }
        let draft = self
            .state
            .drafts
            .iter_mut()
            .find(|draft| &draft.definition_id == definition_id)
            .expect("draft was found above while the workspace lock is held");
        if draft.save_prompted_hash.as_deref() == Some(content_hash) {
            return Ok(false);
        }
        draft.save_prompted_hash = Some(content_hash.to_string());
        draft.content_hash = content_hash.to_string();
        self.persist()?;
        Ok(true)
    }

    pub(crate) fn discard(
        &mut self,
        definition_id: &WorkflowDefinitionId,
    ) -> Result<(), WorkspaceError> {
        let _lock = self.lock_state()?;
        self.reload_state()?;
        let index = self
            .state
            .drafts
            .iter()
            .position(|draft| &draft.definition_id == definition_id)
            .ok_or_else(|| WorkspaceError::NotDraft(definition_id.0.clone()))?;
        let draft = self.state.drafts.remove(index);
        let script_file = draft.script_file.clone();
        if self.state.focused_definition_id.as_ref() == Some(definition_id) {
            self.state.focused_definition_id = match draft.source {
                DraftSource::Definition { definition_id, .. } => Some(definition_id),
                DraftSource::Inline | DraftSource::File { .. } => None,
            };
        }
        self.persist()?;
        if let Err(error) = self.remove_draft_file(&script_file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %self.root.display_path().join(&script_file).display(), %error, "failed to remove discarded workflow draft");
        }
        Ok(())
    }

    fn definition_from_saved(&self, resolved: ResolvedWorkflow) -> WorkflowDefinition {
        let path = match &resolved.source {
            WorkflowSource::File(path) => Some(path.display().to_string()),
            WorkflowSource::Builtin | WorkflowSource::Inline => None,
        };
        let validated = self
            .state
            .validated_hashes
            .get(&resolved.definition_id)
            .is_some_and(|hash| hash == &resolved.content_hash);
        let summary = WorkflowDefinitionSummary {
            focused: self
                .state
                .focused_definition_id
                .as_ref()
                .is_some_and(|focused| focused == &resolved.definition_id),
            definition_id: resolved.definition_id.clone(),
            name: resolved.meta.name.clone(),
            description: resolved.meta.description.clone(),
            when_to_use: resolved.meta.when_to_use.clone(),
            scope: resolved.scope,
            status: if validated {
                "saved,validated".into()
            } else {
                "saved".into()
            },
            path,
            draft_source: None,
            source_definition_id: None,
            source_path: None,
            content_hash: resolved.content_hash.clone(),
        };
        WorkflowDefinition { summary, resolved }
    }

    fn definition_from_draft(
        &self,
        draft: &DraftRecord,
    ) -> Result<WorkflowDefinition, WorkspaceError> {
        let path = self.draft_display_path(draft);
        let script = self.read_draft(draft)?;
        let meta = registry::parse_workflow(&script, None)?;
        if meta.name != draft.name {
            return Err(WorkspaceError::InvalidState(format!(
                "draft {} meta.name changed to '{}'",
                draft.name, meta.name
            )));
        }
        let hash = registry::content_hash(&script);
        let dirty = match &draft.source {
            DraftSource::Definition { baseline_hash, .. } => baseline_hash != &hash,
            DraftSource::Inline | DraftSource::File { .. } => true,
        };
        let validated = draft.last_validated_hash.as_deref() == Some(hash.as_str());
        let mut states = vec!["temporary"];
        if dirty {
            states.push("dirty");
        }
        if validated {
            states.push("validated");
        }
        if draft.conflicted {
            states.push("conflicted");
        }
        let resolved = ResolvedWorkflow {
            definition_id: draft.definition_id.clone(),
            scope: WorkflowScope::Session,
            content_hash: hash.clone(),
            private: false,
            meta,
            script,
            source: WorkflowSource::File(path.clone()),
        };
        let (draft_source, source_definition_id, source_path) = match &draft.source {
            DraftSource::Inline => ("inline", None, None),
            DraftSource::File { path } => ("file", None, Some(path.clone())),
            DraftSource::Definition {
                definition_id,
                path,
                ..
            } => ("definition", Some(definition_id.clone()), path.clone()),
        };
        let summary = WorkflowDefinitionSummary {
            focused: self
                .state
                .focused_definition_id
                .as_ref()
                .is_some_and(|focused| focused == &draft.definition_id),
            definition_id: draft.definition_id.clone(),
            name: draft.name.clone(),
            description: resolved.meta.description.clone(),
            when_to_use: resolved.meta.when_to_use.clone(),
            scope: WorkflowScope::Session,
            status: states.join(","),
            path: Some(path.display().to_string()),
            draft_source: Some(draft_source.into()),
            source_definition_id,
            source_path,
            content_hash: hash,
        };
        Ok(WorkflowDefinition { summary, resolved })
    }

    fn draft_display_path(&self, draft: &DraftRecord) -> std::path::PathBuf {
        self.root.display_path().join(&draft.script_file)
    }

    fn open_drafts(
        &self,
        create_missing: bool,
    ) -> Result<crate::session::storage::ContainedDirectory, WorkspaceError> {
        self.root
            .open_relative(
                Path::new("drafts"),
                "Workflow draft directory",
                create_missing,
            )
            .map_err(|error| WorkspaceError::Io {
                path: self
                    .root
                    .display_path()
                    .join("drafts")
                    .display()
                    .to_string(),
                error: error.to_string(),
            })
    }

    fn draft_name(script_file: &str) -> Result<&std::ffi::OsStr, WorkspaceError> {
        let path = Path::new(script_file);
        if path.parent() != Some(Path::new("drafts"))
            || path.components().count() != 2
            || path.file_name().is_none()
        {
            return Err(WorkspaceError::InvalidState(format!(
                "invalid Workflow draft path: {script_file}"
            )));
        }
        Ok(path.file_name().expect("checked above"))
    }

    fn read_draft(&self, draft: &DraftRecord) -> Result<String, WorkspaceError> {
        let name = Self::draft_name(&draft.script_file)?;
        let drafts = self.open_drafts(false)?;
        let bytes = drafts
            .read_bounded(name, "Workflow draft", registry::MAX_WORKFLOW_SOURCE_BYTES)
            .map_err(|error| WorkspaceError::Io {
                path: self.draft_display_path(draft).display().to_string(),
                error: error.to_string(),
            })?;
        String::from_utf8(bytes).map_err(|error| {
            WorkspaceError::InvalidState(format!(
                "{} is not valid UTF-8: {error}",
                self.draft_display_path(draft).display()
            ))
        })
    }

    fn remove_draft_file(&self, script_file: &str) -> std::io::Result<()> {
        let name = Self::draft_name(script_file).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;
        self.root
            .open_relative(Path::new("drafts"), "Workflow draft directory", false)?
            .remove_file(name, true)
    }

    fn write_draft(&self, draft: &DraftRecord, script: &str) -> Result<(), WorkspaceError> {
        let name = Self::draft_name(&draft.script_file)?;
        let drafts = self.open_drafts(true)?;
        drafts
            .write_atomic(name, script.as_bytes(), true, true)
            .map_err(|error| WorkspaceError::Io {
                path: self.draft_display_path(draft).display().to_string(),
                error: error.to_string(),
            })
    }

    fn persist(&self) -> Result<(), WorkspaceError> {
        let bytes = serde_json::to_vec_pretty(&self.state)
            .map_err(|error| WorkspaceError::InvalidState(error.to_string()))?;
        self.root
            .write_atomic(std::ffi::OsStr::new("state.json"), &bytes, true, true)
            .map_err(|error| WorkspaceError::Io {
                path: self
                    .root
                    .display_path()
                    .join("state.json")
                    .display()
                    .to_string(),
                error: error.to_string(),
            })
    }

    fn lock_state(&self) -> Result<std::fs::File, WorkspaceError> {
        use fs2::FileExt as _;

        let path = self.root.display_path().join("state.lock");
        let file = self
            .root
            .open_read_write_create(std::ffi::OsStr::new("state.lock"))
            .map_err(|error| WorkspaceError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            })?;
        file.lock_exclusive().map_err(|error| WorkspaceError::Io {
            path: path.display().to_string(),
            error: error.to_string(),
        })?;
        Ok(file)
    }

    fn reload_state(&mut self) -> Result<(), WorkspaceError> {
        self.state = load_state(&self.root)?.unwrap_or_default();
        Ok(())
    }

    fn recover_pending_publish(&mut self, cwd: &Path) -> Result<(), WorkspaceError> {
        if self.state.pending_publish.is_none() {
            return Ok(());
        }
        let _lock = self.lock_state()?;
        self.reload_state()?;
        self.resolve_pending_publish(cwd).map(|_| ())
    }

    /// Resolve a write-ahead publish intent from filesystem truth. A matching
    /// target hash means the atomic target commit completed; an unchanged
    /// baseline or absent new target means it did not. Any third value is an
    /// external conflict and leaves the draft available for inspection.
    fn resolve_pending_publish(&mut self, cwd: &Path) -> Result<bool, WorkspaceError> {
        let Some(pending) = self.state.pending_publish.clone() else {
            return Ok(false);
        };
        let target_name = pending
            .target_definition_id
            .0
            .split_once(':')
            .map(|(_, name)| name)
            .ok_or_else(|| {
                WorkspaceError::InvalidState("publish target Definition id is invalid".into())
            })?;
        let target = match registry::validate_publish_target_path(
            cwd,
            pending.scope,
            target_name,
            Path::new(&pending.target_path),
        ) {
            Ok(target) => target,
            Err(error) => {
                if let Some(draft) = self
                    .state
                    .drafts
                    .iter_mut()
                    .find(|draft| draft.definition_id == pending.draft_definition_id)
                {
                    draft.conflicted = true;
                }
                self.state.pending_publish = None;
                self.persist()?;
                tracing::warn!(%error, "discarded an untrusted pending Workflow publish target");
                return Ok(false);
            }
        };
        let target_script = registry::read_publish_target(cwd, pending.scope, target_name)?;
        let actual_hash = target_script.as_deref().map(registry::content_hash);
        let target_missing = target_script.is_none();
        if actual_hash.as_deref() == Some(pending.content_hash.as_str()) {
            let draft_path = self.complete_pending_publish()?;
            self.persist()?;
            if let Some(script_file) = draft_path
                && let Err(error) = self.remove_draft_file(&script_file)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path = %self.root.display_path().join(&script_file).display(), %error, "failed to remove recovered published workflow draft");
            }
            return Ok(true);
        }

        let unchanged = match pending.expected_base_hash.as_deref() {
            Some(expected) => actual_hash.as_deref() == Some(expected),
            None => target_missing,
        };
        if !unchanged
            && let Some(draft) = self
                .state
                .drafts
                .iter_mut()
                .find(|draft| draft.definition_id == pending.draft_definition_id)
        {
            draft.conflicted = true;
        }
        self.state.pending_publish = None;
        self.persist()?;
        Ok(false)
    }

    /// Commit Workspace state after the target file is durable. If an external
    /// editor changed the draft after the publish intent captured its hash,
    /// retain that newer content as a draft derived from the just-published
    /// Definition instead of deleting it.
    fn complete_pending_publish(&mut self) -> Result<Option<String>, WorkspaceError> {
        let pending = self.state.pending_publish.take().ok_or_else(|| {
            WorkspaceError::InvalidState("publish intent disappeared before commit".into())
        })?;
        let index = self
            .state
            .drafts
            .iter()
            .position(|draft| draft.definition_id == pending.draft_definition_id)
            .ok_or_else(|| {
                WorkspaceError::InvalidState(
                    "publish intent references a missing session draft".into(),
                )
            })?;
        let script_file = self.state.drafts[index].script_file.clone();
        let current_hash = self
            .read_draft(&self.state.drafts[index])
            .ok()
            .map(|script| registry::content_hash(&script));
        let remove_draft = current_hash.as_deref() == Some(pending.content_hash.as_str());
        let draft_unreadable = current_hash.is_none();
        if remove_draft {
            self.state.drafts.remove(index);
            self.state.focused_definition_id = Some(pending.target_definition_id.clone());
        } else {
            let draft = &mut self.state.drafts[index];
            if let Some(current_hash) = current_hash {
                draft.content_hash = current_hash.clone();
                if draft.last_validated_hash.as_deref() != Some(current_hash.as_str()) {
                    draft.last_validated_hash = None;
                }
                if draft.save_prompted_hash.as_deref() != Some(current_hash.as_str()) {
                    draft.save_prompted_hash = None;
                }
            }
            draft.source = DraftSource::Definition {
                definition_id: pending.target_definition_id.clone(),
                scope: pending.scope,
                path: Some(pending.target_path.clone()),
                baseline_hash: pending.content_hash.clone(),
            };
            draft.conflicted = draft_unreadable;
            self.state.focused_definition_id = Some(draft.definition_id.clone());
        }
        self.state
            .validated_hashes
            .insert(pending.target_definition_id, pending.content_hash);
        Ok(remove_draft.then_some(script_file))
    }

    fn refresh_content_hashes(&mut self) -> Result<(), WorkspaceError> {
        if self.state.drafts.is_empty() {
            return Ok(());
        }
        let _lock = self.lock_state()?;
        self.reload_state()?;
        let mut changed = false;
        for index in 0..self.state.drafts.len() {
            let Ok(script) = self.read_draft(&self.state.drafts[index]) else {
                continue;
            };
            let hash = registry::content_hash(&script);
            if self.state.drafts[index].content_hash != hash {
                self.state.drafts[index].content_hash = hash;
                changed = true;
            }
        }
        if changed {
            self.persist()?;
        }
        Ok(())
    }
}

fn load_state(
    root: &crate::session::storage::ContainedDirectory,
) -> Result<Option<WorkspaceState>, WorkspaceError> {
    let path = root.display_path().join("state.json");
    let bytes = match root.read_bounded(
        std::ffi::OsStr::new("state.json"),
        "Workflow workspace state",
        MAX_WORKSPACE_STATE_BYTES,
    ) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(WorkspaceError::Io {
                path: path.display().to_string(),
                error: error.to_string(),
            });
        }
    };
    let state: WorkspaceState = serde_json::from_slice(&bytes)
        .map_err(|error| WorkspaceError::InvalidState(format!("{}: {error}", path.display())))?;
    validate_state(&state)?;
    Ok(Some(state))
}

fn validate_state(state: &WorkspaceState) -> Result<(), WorkspaceError> {
    if state.version != WORKSPACE_VERSION {
        return Err(WorkspaceError::InvalidState(format!(
            "unsupported version {}",
            state.version
        )));
    }
    let valid_hash =
        |value: &str| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    let valid_id = |id: &WorkflowDefinitionId| {
        let Some((scope, local)) = id.0.split_once(':') else {
            return false;
        };
        matches!(scope, "session" | "project" | "user" | "builtin")
            && !local.is_empty()
            && local
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    };
    if state
        .focused_definition_id
        .as_ref()
        .is_some_and(|id| !valid_id(id))
    {
        return Err(WorkspaceError::InvalidState(
            "focused Definition id is invalid".into(),
        ));
    }
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for draft in &state.drafts {
        registry::validate_workflow_name(&draft.name)
            .map_err(|error| WorkspaceError::InvalidState(error.to_string()))?;
        let Some(local_id) = draft.definition_id.0.strip_prefix("session:") else {
            return Err(WorkspaceError::InvalidState(
                "draft Definition id must use session scope".into(),
            ));
        };
        if !valid_id(&draft.definition_id)
            || !ids.insert(draft.definition_id.clone())
            || !names.insert(draft.name.as_str())
        {
            return Err(WorkspaceError::InvalidState(
                "draft Definition ids and names must be unique and valid".into(),
            ));
        }
        if draft.script_file != format!("drafts/{local_id}.rhai") {
            return Err(WorkspaceError::InvalidState(format!(
                "draft '{}' has an invalid script path",
                draft.name
            )));
        }
        if !valid_hash(&draft.content_hash) {
            return Err(WorkspaceError::InvalidState(format!(
                "draft '{}' has an invalid current content hash",
                draft.name
            )));
        }
        for hash in [
            draft.last_validated_hash.as_deref(),
            draft.save_prompted_hash.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if !valid_hash(hash) {
                return Err(WorkspaceError::InvalidState(format!(
                    "draft '{}' has an invalid content hash",
                    draft.name
                )));
            }
        }
        match &draft.source {
            DraftSource::Inline => {}
            DraftSource::File { path } if path.trim().is_empty() => {
                return Err(WorkspaceError::InvalidState(format!(
                    "draft '{}' has an invalid file source",
                    draft.name
                )));
            }
            DraftSource::File { .. } => {}
            DraftSource::Definition {
                definition_id,
                scope,
                baseline_hash,
                ..
            } if *scope == WorkflowScope::Session
                || !valid_id(definition_id)
                || !definition_id.0.starts_with(&format!("{}:", scope.as_str()))
                || !valid_hash(baseline_hash) =>
            {
                return Err(WorkspaceError::InvalidState(format!(
                    "draft '{}' has an invalid Definition source",
                    draft.name
                )));
            }
            DraftSource::Definition { .. } => {}
        }
    }
    for (definition_id, hash) in &state.validated_hashes {
        if !valid_id(definition_id) || !valid_hash(hash) {
            return Err(WorkspaceError::InvalidState(
                "saved Definition validation cache is invalid".into(),
            ));
        }
    }
    if let Some(pending) = &state.pending_publish {
        let pending_draft = state
            .drafts
            .iter()
            .find(|draft| draft.definition_id == pending.draft_definition_id);
        if !matches!(pending.scope, WorkflowScope::Project | WorkflowScope::User)
            || !valid_id(&pending.draft_definition_id)
            || !pending.draft_definition_id.0.starts_with("session:")
            || !valid_id(&pending.target_definition_id)
            || !pending
                .target_definition_id
                .0
                .starts_with(&format!("{}:", pending.scope.as_str()))
            || !valid_hash(&pending.content_hash)
            || pending
                .expected_base_hash
                .as_deref()
                .is_some_and(|hash| !valid_hash(hash))
            || pending.target_path.trim().is_empty()
            || pending_draft.is_none()
            || pending_draft.is_some_and(|draft| {
                pending.target_definition_id != registry::definition_id(pending.scope, &draft.name)
            })
        {
            return Err(WorkspaceError::InvalidState(
                "pending Workflow publish intent is invalid".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(name: &str, description: &str) -> String {
        format!(
            "let meta = #{{ name: \"{name}\", description: \"{description}\" }};\ncomplete(\"ok\");"
        )
    }

    #[test]
    fn persists_multiple_drafts_with_one_explicit_focus() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let first = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("first", "one"),
                },
            )
            .unwrap();
        let second = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("second", "two"),
                },
            )
            .unwrap();
        assert!(second.summary.focused);

        let restored = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        assert!(
            !restored
                .resolve(project.path(), &first.summary.definition_id)
                .unwrap()
                .summary
                .focused
        );
        let catalog = restored.catalog(project.path());
        assert_eq!(
            catalog
                .definitions
                .iter()
                .filter(|definition| definition.scope == WorkflowScope::Session)
                .count(),
            2
        );
        assert_eq!(
            catalog
                .definitions
                .iter()
                .filter(|definition| definition.focused)
                .count(),
            1
        );
    }

    #[test]
    fn search_ignores_an_unrelated_focus_and_ranks_exact_metadata_matches() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let review = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("review-changes", "review a patch"),
                },
            )
            .unwrap();
        workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("deploy-release", "ship a release"),
                },
            )
            .unwrap();

        let matches = workspace.search(project.path(), "review-changes", 10);
        assert_eq!(matches.definitions.len(), 1);
        assert_eq!(
            matches.definitions[0].definition_id,
            review.summary.definition_id
        );
        assert!(!matches.definitions[0].focused);
        assert!(
            workspace
                .search(project.path(), "no-match", 10)
                .definitions
                .is_empty()
        );
    }

    #[test]
    fn publish_requires_current_hash_validation() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let draft = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("publish-me", "one"),
                },
            )
            .unwrap();
        assert!(matches!(
            workspace.publish(
                project.path(),
                &draft.summary.definition_id,
                WorkflowScope::Project
            ),
            Err(WorkspaceError::NotValidated)
        ));
        workspace
            .record_validated(
                project.path(),
                &draft.summary.definition_id,
                &draft.summary.content_hash,
            )
            .unwrap();
        let published = workspace
            .publish(
                project.path(),
                &draft.summary.definition_id,
                WorkflowScope::Project,
            )
            .unwrap();
        assert_eq!(published.summary.scope, WorkflowScope::Project);
        assert!(published.summary.focused);
    }

    #[test]
    fn successful_hash_prompts_to_save_only_once() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let draft = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("prompt-once", "one"),
                },
            )
            .unwrap();
        let mut stale_workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        assert!(
            workspace
                .take_save_prompt(&draft.summary.definition_id, &draft.summary.content_hash)
                .unwrap()
        );
        assert!(
            !stale_workspace
                .take_save_prompt(&draft.summary.definition_id, &draft.summary.content_hash)
                .unwrap()
        );

        let draft_path = std::path::PathBuf::from(draft.summary.path.clone().unwrap());
        let changed_script = script("prompt-once", "changed");
        std::fs::write(&draft_path, &changed_script).unwrap();
        let changed_hash = registry::content_hash(&changed_script);
        let refreshed = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        assert_eq!(refreshed.state.drafts[0].content_hash, changed_hash);
        assert!(
            !workspace
                .take_save_prompt(&draft.summary.definition_id, &draft.summary.content_hash)
                .unwrap(),
            "completion of an old Run snapshot must not prompt for changed draft content"
        );
        assert!(
            workspace
                .take_save_prompt(&draft.summary.definition_id, &changed_hash)
                .unwrap(),
            "the changed hash may prompt after its own successful Run"
        );
    }

    #[test]
    fn derived_publish_detects_external_source_change() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        registry::publish_workflow(
            project.path(),
            WorkflowScope::Project,
            "conflict",
            &script("conflict", "baseline"),
            None,
        )
        .unwrap();
        let source_id = registry::definition_id(WorkflowScope::Project, "conflict");
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let draft = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Definition {
                    definition_id: source_id,
                },
            )
            .unwrap();
        workspace
            .record_validated(
                project.path(),
                &draft.summary.definition_id,
                &draft.summary.content_hash,
            )
            .unwrap();
        let source_path = project.path().join(".grow/workflows/conflict.rhai");
        std::fs::write(&source_path, script("conflict", "external edit")).unwrap();
        assert!(matches!(
            workspace.publish(
                project.path(),
                &draft.summary.definition_id,
                WorkflowScope::Project,
            ),
            Err(WorkspaceError::Resolve(
                ResolveError::PublishConflict { .. }
            ))
        ));
        let conflicted = workspace
            .resolve(project.path(), &draft.summary.definition_id)
            .unwrap();
        assert!(conflicted.summary.status.contains("conflicted"));
    }

    #[test]
    fn open_recovers_a_target_commit_that_preceded_workspace_commit() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let draft = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("recover-publish", "committed"),
                },
            )
            .unwrap();
        let target_id = registry::definition_id(WorkflowScope::Project, "recover-publish");
        let target_path = registry::publish_target_path(
            project.path(),
            WorkflowScope::Project,
            "recover-publish",
        )
        .unwrap();
        workspace.state.pending_publish = Some(PendingPublish {
            draft_definition_id: draft.summary.definition_id.clone(),
            target_definition_id: target_id.clone(),
            scope: WorkflowScope::Project,
            target_path: target_path.display().to_string(),
            content_hash: draft.summary.content_hash.clone(),
            expected_base_hash: None,
        });
        workspace.persist().unwrap();
        registry::publish_workflow(
            project.path(),
            WorkflowScope::Project,
            "recover-publish",
            &draft.resolved.script,
            None,
        )
        .unwrap();

        let restored = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        assert!(restored.state.pending_publish.is_none());
        assert!(restored.state.drafts.is_empty());
        assert_eq!(restored.state.focused_definition_id, Some(target_id));
        assert!(target_path.exists());
    }

    #[test]
    fn publish_recovery_preserves_a_draft_edited_after_target_commit() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        let draft = workspace
            .draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("recover-edited", "published"),
                },
            )
            .unwrap();
        let target_id = registry::definition_id(WorkflowScope::Project, "recover-edited");
        let target_path =
            registry::publish_target_path(project.path(), WorkflowScope::Project, "recover-edited")
                .unwrap();
        workspace.state.pending_publish = Some(PendingPublish {
            draft_definition_id: draft.summary.definition_id.clone(),
            target_definition_id: target_id.clone(),
            scope: WorkflowScope::Project,
            target_path: target_path.display().to_string(),
            content_hash: draft.summary.content_hash.clone(),
            expected_base_hash: None,
        });
        workspace.persist().unwrap();
        registry::publish_workflow(
            project.path(),
            WorkflowScope::Project,
            "recover-edited",
            &draft.resolved.script,
            None,
        )
        .unwrap();
        std::fs::write(
            draft.summary.path.as_deref().unwrap(),
            script("recover-edited", "newer draft edit"),
        )
        .unwrap();

        let restored = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        assert!(restored.state.pending_publish.is_none());
        assert_eq!(restored.state.drafts.len(), 1);
        assert_eq!(
            restored.state.focused_definition_id,
            Some(draft.summary.definition_id.clone())
        );
        let current = restored
            .resolve(project.path(), &draft.summary.definition_id)
            .unwrap();
        assert!(current.summary.status.contains("dirty"));
        assert_eq!(current.summary.source_definition_id, Some(target_id));
    }

    #[test]
    fn workspace_rejects_state_with_an_escaping_draft_path() {
        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = session.path().join("workflow-workspace");
        std::fs::create_dir_all(&root).unwrap();
        let state = WorkspaceState {
            version: WORKSPACE_VERSION,
            focused_definition_id: None,
            drafts: vec![DraftRecord {
                definition_id: WorkflowDefinitionId::new("session:abc"),
                name: "escape".into(),
                script_file: "../../outside.rhai".into(),
                content_hash: registry::content_hash(""),
                source: DraftSource::Inline,
                last_validated_hash: None,
                save_prompted_hash: None,
                conflicted: false,
            }],
            validated_hashes: Default::default(),
            pending_publish: None,
        };
        std::fs::write(root.join("state.json"), serde_json::to_vec(&state).unwrap()).unwrap();
        assert!(matches!(
            WorkflowWorkspace::open(session.path(), project.path()),
            Err(WorkspaceError::InvalidState(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_rejects_symlinked_root_and_draft_directory() {
        use std::os::unix::fs::symlink;

        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        symlink(external.path(), session.path().join("workflow-workspace")).unwrap();
        assert!(matches!(
            WorkflowWorkspace::open(session.path(), project.path()),
            Err(WorkspaceError::InvalidState(_))
        ));

        let state_session = tempfile::tempdir().unwrap();
        let state_root = state_session.path().join("workflow-workspace");
        std::fs::create_dir_all(&state_root).unwrap();
        let external_state = external.path().join("state.json");
        std::fs::write(&external_state, b"{}").unwrap();
        symlink(&external_state, state_root.join("state.json")).unwrap();
        assert!(matches!(
            WorkflowWorkspace::open(state_session.path(), project.path()),
            Err(WorkspaceError::InvalidState(_))
        ));

        let session = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let root = session.path().join("workflow-workspace");
        std::fs::create_dir_all(&root).unwrap();
        let mut workspace = WorkflowWorkspace::open(session.path(), project.path()).unwrap();
        symlink(external.path(), root.join("drafts")).unwrap();
        assert!(matches!(
            workspace.draft(
                project.path(),
                None,
                WorkflowDraftSource::Inline {
                    script: script("contained", "one"),
                },
            ),
            Err(WorkspaceError::InvalidState(_))
        ));
        assert_eq!(std::fs::read_dir(external.path()).unwrap().count(), 1);
    }
}
