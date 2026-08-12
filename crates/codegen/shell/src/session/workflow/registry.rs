use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use tools::implementations::grow_build::workflow::{
    WorkflowDefinitionId, WorkflowDiagnostic, WorkflowScope,
};
use workflow::{WorkflowMeta, extract_meta};

pub(crate) const MAX_WORKFLOW_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_WORKFLOW_NAME_BYTES: usize = 64;

pub(crate) struct BuiltinWorkflow {
    pub name: &'static str,
    pub script: &'static str,
}

pub(crate) const BUILTIN_WORKFLOWS: &[BuiltinWorkflow] = &[];
const DEEP_RESEARCH_SCRIPT: &str = include_str!("../workflows/deep_research.rhai");

pub(crate) struct ResolvedWorkflow {
    pub definition_id: WorkflowDefinitionId,
    pub scope: WorkflowScope,
    pub content_hash: String,
    pub private: bool,
    pub meta: WorkflowMeta,
    pub script: String,
    pub source: WorkflowSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkflowSource {
    Builtin,
    Inline,
    File(PathBuf),
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ResolveError {
    #[error("unknown workflow: {0}")]
    UnknownName(String),
    #[error(
        "invalid workflow name '{0}': expected 1-64 lowercase letters, digits, or single hyphens"
    )]
    InvalidName(String),
    #[error("ambiguous workflow '{name}': duplicate definitions in {scope} scope")]
    DuplicateName { name: String, scope: &'static str },
    #[error("workflow path is not trusted: {path} ({reason})")]
    UntrustedPath { path: String, reason: String },
    #[error("workflow source exceeds {limit} bytes: {path}")]
    SourceTooLarge { path: String, limit: u64 },
    #[error("invalid workflow filename '{0}': expected <safe-name>.rhai")]
    InvalidFilename(String),
    #[error("saved workflow filename '{filename}' must match meta.name '{name}'")]
    FilenameMismatch { filename: String, name: String },
    #[error("failed to read {path}: {error}")]
    Io { path: String, error: String },
    #[error("invalid workflow script: {0}")]
    Meta(#[from] workflow::MetaError),
    #[error("workflow publish conflict at {path}: {reason}")]
    PublishConflict { path: String, reason: String },
}

pub(crate) fn project_root(session_cwd: &Path) -> PathBuf {
    workspace::session::git::find_git_root_from_path(session_cwd)
        .unwrap_or_else(|_| session_cwd.to_path_buf())
}

pub(crate) fn user_workflow_dir() -> PathBuf {
    crate::util::grow_home::grow_home().join("workflows")
}

pub(crate) struct WorkflowRegistry {
    entries: Vec<RegistryEntry>,
    duplicate_names: BTreeMap<String, &'static str>,
    diagnostics: Vec<WorkflowDiagnostic>,
}

struct RegistryEntry {
    definition_id: WorkflowDefinitionId,
    scope: WorkflowScope,
    meta: WorkflowMeta,
    script: String,
    source: WorkflowSource,
    source_label: &'static str,
    path: Option<PathBuf>,
}

fn builtin_meta_cache() -> &'static [(WorkflowMeta, &'static BuiltinWorkflow)] {
    static CACHE: std::sync::OnceLock<Vec<(WorkflowMeta, &'static BuiltinWorkflow)>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        BUILTIN_WORKFLOWS
            .iter()
            .filter_map(|builtin| {
                let meta = parse_workflow(builtin.script, None).ok()?;
                (meta.name == builtin.name && is_valid_workflow_name(builtin.name))
                    .then_some((meta, builtin))
            })
            .collect()
    })
}

pub(crate) fn warm_builtin_cache() {
    let _ = builtin_meta_cache();
}

fn cached_builtin_entries() -> Vec<RegistryEntry> {
    builtin_meta_cache()
        .iter()
        .map(|(meta, builtin)| RegistryEntry {
            definition_id: definition_id(WorkflowScope::Builtin, &meta.name),
            scope: WorkflowScope::Builtin,
            meta: meta.clone(),
            script: builtin.script.to_string(),
            source: WorkflowSource::Builtin,
            source_label: "builtin",
            path: None,
        })
        .collect()
}

impl WorkflowRegistry {
    pub(crate) fn scan(session_cwd: Option<&Path>) -> Self {
        let mut entries = Vec::new();
        let mut duplicate_names = BTreeMap::new();
        let mut diagnostics = Vec::new();

        let mut dirs = Vec::new();
        if let Some(cwd) = session_cwd {
            let project_dir = project_root(cwd).join(".grow").join("workflows");
            if crate::agent::folder_trust::project_scope_allowed(cwd) {
                dirs.push((project_dir, "project", WorkflowScope::Project));
            } else if project_dir.exists() {
                diagnostics.push(WorkflowDiagnostic {
                    scope: WorkflowScope::Project,
                    path: Some(project_dir.display().to_string()),
                    code: "untrusted_path".into(),
                    message: "project Workflow Definitions are hidden until the folder is trusted"
                        .into(),
                });
            }
        }
        dirs.push((user_workflow_dir(), "user", WorkflowScope::User));

        for (dir, source_label, scope) in dirs {
            let (mut scoped, mut scoped_diagnostics) = scan_directory(&dir, source_label, scope);
            reject_same_scope_duplicates(
                &mut scoped,
                source_label,
                scope,
                &mut duplicate_names,
                &mut scoped_diagnostics,
            );
            entries.extend(scoped);
            diagnostics.append(&mut scoped_diagnostics);
        }

        let mut builtin_entries = cached_builtin_entries();
        reject_same_scope_duplicates(
            &mut builtin_entries,
            "builtin",
            WorkflowScope::Builtin,
            &mut duplicate_names,
            &mut diagnostics,
        );
        entries.extend(builtin_entries);

        Self {
            entries,
            duplicate_names,
            diagnostics,
        }
    }

    pub(crate) fn resolve_by_name(&self, name: &str) -> Result<ResolvedWorkflow, ResolveError> {
        validate_workflow_name(name)?;
        Ok(resolved_entry(self.resolve_entry(name)?))
    }

    pub(crate) fn resolve_by_id(
        &self,
        definition_id: &WorkflowDefinitionId,
    ) -> Result<ResolvedWorkflow, ResolveError> {
        self.entries
            .iter()
            .find(|entry| &entry.definition_id == definition_id)
            .map(resolved_entry)
            .ok_or_else(|| ResolveError::UnknownName(definition_id.0.clone()))
    }

    fn resolve_entry(&self, name: &str) -> Result<&RegistryEntry, ResolveError> {
        if let Some(scope) = self.duplicate_names.get(name) {
            return Err(ResolveError::DuplicateName {
                name: name.to_string(),
                scope,
            });
        }
        self.entries
            .iter()
            .find(|entry| entry.meta.name == name)
            .ok_or_else(|| ResolveError::UnknownName(name.to_string()))
    }

    pub(crate) fn list(&self) -> Vec<WorkflowListing> {
        self.entries
            .iter()
            .map(|entry| WorkflowListing {
                definition_id: entry.definition_id.clone(),
                name: entry.meta.name.clone(),
                description: entry.meta.description.clone(),
                when_to_use: entry.meta.when_to_use.clone(),
                source: entry.source_label,
                scope: entry.scope,
                path: entry
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned()),
                status: "saved".into(),
                content_hash: content_hash(&entry.script),
                focused: false,
            })
            .collect()
    }

    pub(crate) fn diagnostics(&self) -> &[WorkflowDiagnostic] {
        &self.diagnostics
    }
}

fn resolved_entry(entry: &RegistryEntry) -> ResolvedWorkflow {
    ResolvedWorkflow {
        definition_id: entry.definition_id.clone(),
        scope: entry.scope,
        content_hash: content_hash(&entry.script),
        private: false,
        meta: entry.meta.clone(),
        script: entry.script.clone(),
        source: entry.source.clone(),
    }
}

fn name_counts(entries: &[RegistryEntry]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entry in entries {
        *counts.entry(entry.meta.name.clone()).or_default() += 1;
    }
    counts
}

fn reject_same_scope_duplicates(
    entries: &mut Vec<RegistryEntry>,
    scope: &'static str,
    workflow_scope: WorkflowScope,
    duplicates: &mut BTreeMap<String, &'static str>,
    diagnostics: &mut Vec<WorkflowDiagnostic>,
) {
    for (name, count) in name_counts(entries) {
        if count > 1 {
            duplicates.insert(name.clone(), scope);
            diagnostics.push(WorkflowDiagnostic {
                scope: workflow_scope,
                path: None,
                code: "duplicate_name".into(),
                message: format!(
                    "workflow '{name}' has {count} definitions in the same {scope} scope"
                ),
            });
            entries.retain(|entry| entry.meta.name != name);
        }
    }
}

fn scan_directory(
    dir: &Path,
    source_label: &'static str,
    scope: WorkflowScope,
) -> (Vec<RegistryEntry>, Vec<WorkflowDiagnostic>) {
    let Ok(dir_meta) = std::fs::symlink_metadata(dir) else {
        return (Vec::new(), Vec::new());
    };
    if dir_meta.file_type().is_symlink() || !dir_meta.is_dir() {
        return (
            Vec::new(),
            vec![WorkflowDiagnostic {
                scope,
                path: Some(dir.display().to_string()),
                code: "untrusted_directory".into(),
                message: "workflow directory must be a real directory, not a symlink".into(),
            }],
        );
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return (
            Vec::new(),
            vec![WorkflowDiagnostic {
                scope,
                path: Some(dir.display().to_string()),
                code: "read_directory_failed".into(),
                message: "workflow directory could not be read".into(),
            }],
        );
    };
    let mut paths: Vec<PathBuf> = read_dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rhai"))
        .collect();
    paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));

    let mut entries = Vec::new();
    let mut diagnostics = Vec::new();
    for path in paths {
        let result = read_trusted_source(&path)
            .and_then(|script| parse_workflow(&script, Some(&path)).map(|meta| (script, meta)));
        match result {
            Ok((script, meta)) => entries.push(RegistryEntry {
                definition_id: definition_id(scope, &meta.name),
                scope,
                meta,
                script,
                source: WorkflowSource::File(path.clone()),
                source_label,
                path: Some(path),
            }),
            Err(error) => diagnostics.push(WorkflowDiagnostic {
                scope,
                path: Some(path.display().to_string()),
                code: diagnostic_code(&error).into(),
                message: error.to_string(),
            }),
        }
    }
    (entries, diagnostics)
}

pub(crate) fn resolve_by_name(
    name: &str,
    session_cwd: Option<&Path>,
) -> Result<ResolvedWorkflow, ResolveError> {
    WorkflowRegistry::scan(session_cwd).resolve_by_name(name)
}

/// Resolve the private workflow backing Deep Research Behavior. It is not
/// inserted into the public registry, so `/workflow-run deep-research` cannot
/// bypass the Behavior lifecycle or its report contract.
pub(crate) fn resolve_deep_research() -> Result<ResolvedWorkflow, ResolveError> {
    let meta = parse_workflow(DEEP_RESEARCH_SCRIPT, None)?;
    Ok(ResolvedWorkflow {
        definition_id: definition_id(WorkflowScope::Builtin, &meta.name),
        scope: WorkflowScope::Builtin,
        content_hash: content_hash(DEEP_RESEARCH_SCRIPT),
        private: true,
        meta,
        script: DEEP_RESEARCH_SCRIPT.to_string(),
        source: WorkflowSource::Builtin,
    })
}

pub(crate) fn resolve_by_path(
    path: &Path,
    session_cwd: &Path,
    session_dir: Option<&Path>,
) -> Result<ResolvedWorkflow, ResolveError> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        session_cwd.join(path)
    };
    let path_meta = std::fs::symlink_metadata(&candidate).map_err(|error| ResolveError::Io {
        path: candidate.display().to_string(),
        error: error.to_string(),
    })?;
    if path_meta.file_type().is_symlink() || !path_meta.is_file() {
        return Err(ResolveError::UntrustedPath {
            path: candidate.display().to_string(),
            reason: "expected a non-symlink regular file".into(),
        });
    }
    let canonical = dunce::canonicalize(&candidate).map_err(|error| ResolveError::Io {
        path: candidate.display().to_string(),
        error: error.to_string(),
    })?;

    let project = dunce::canonicalize(project_root(session_cwd)).ok();
    let user_workflows = dunce::canonicalize(user_workflow_dir()).ok();
    let session_runs = session_dir
        .map(|dir| dir.join("workflows"))
        .and_then(|dir| dunce::canonicalize(dir).ok());
    let in_user_or_session = user_workflows
        .as_ref()
        .is_some_and(|root| canonical.starts_with(root))
        || session_runs
            .as_ref()
            .is_some_and(|root| canonical.starts_with(root));
    let in_project = project
        .as_ref()
        .is_some_and(|root| canonical.starts_with(root));
    if in_project
        && !in_user_or_session
        && !crate::agent::folder_trust::project_scope_allowed(session_cwd)
    {
        return Err(ResolveError::UntrustedPath {
            path: candidate.display().to_string(),
            reason: "project workflows require folder trust".into(),
        });
    }
    if !in_project && !in_user_or_session {
        return Err(ResolveError::UntrustedPath {
            path: candidate.display().to_string(),
            reason: "outside the project, grow home, and session workflow runs".into(),
        });
    }

    let script = read_trusted_source(&canonical)?;
    let in_session_runs = session_runs
        .as_ref()
        .is_some_and(|root| canonical.starts_with(root));
    let filename_path = (!in_session_runs).then_some(canonical.as_path());
    let meta = parse_workflow(&script, filename_path)?;
    let scope = if in_session_runs {
        WorkflowScope::Session
    } else if user_workflows
        .as_ref()
        .is_some_and(|root| canonical.starts_with(root))
    {
        WorkflowScope::User
    } else {
        WorkflowScope::Project
    };
    Ok(ResolvedWorkflow {
        definition_id: definition_id(scope, &meta.name),
        scope,
        content_hash: content_hash(&script),
        private: false,
        meta,
        script,
        source: WorkflowSource::File(canonical),
    })
}

pub(crate) fn resolve_inline(script: String) -> Result<ResolvedWorkflow, ResolveError> {
    if script.len() as u64 > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(ResolveError::SourceTooLarge {
            path: "<inline>".into(),
            limit: MAX_WORKFLOW_SOURCE_BYTES,
        });
    }
    let meta = parse_workflow(&script, None)?;
    Ok(ResolvedWorkflow {
        definition_id: WorkflowDefinitionId::new(format!(
            "session:{}",
            uuid::Uuid::now_v7().simple()
        )),
        scope: WorkflowScope::Session,
        content_hash: content_hash(&script),
        private: false,
        meta,
        script,
        source: WorkflowSource::Inline,
    })
}

pub(crate) fn parse_workflow(
    script: &str,
    path: Option<&Path>,
) -> Result<WorkflowMeta, ResolveError> {
    let meta = extract_meta(script)?;
    validate_workflow_name(&meta.name)?;
    if let Some(path) = path {
        validate_workflow_filename(path, &meta.name)?;
    }
    Ok(meta)
}

pub(crate) fn content_hash(script: &str) -> String {
    blake3::hash(script.as_bytes()).to_hex().to_string()
}

pub(crate) fn definition_id(scope: WorkflowScope, name: &str) -> WorkflowDefinitionId {
    WorkflowDefinitionId::new(format!("{}:{name}", scope.as_str()))
}

fn diagnostic_code(error: &ResolveError) -> &'static str {
    match error {
        ResolveError::DuplicateName { .. } => "duplicate_name",
        ResolveError::UntrustedPath { .. } => "untrusted_path",
        ResolveError::SourceTooLarge { .. } => "source_too_large",
        ResolveError::InvalidFilename(_) => "invalid_filename",
        ResolveError::FilenameMismatch { .. } => "filename_mismatch",
        ResolveError::Meta(_) => "invalid_script",
        ResolveError::Io { .. } => "read_failed",
        ResolveError::PublishConflict { .. } => "publish_conflict",
        ResolveError::UnknownName(_) | ResolveError::InvalidName(_) => "invalid_definition",
    }
}

fn validate_workflow_filename(path: &Path, name: &str) -> Result<(), ResolveError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let filename = path
        .file_name()
        .map(|filename| filename.to_string_lossy().into_owned())
        .unwrap_or_default();
    if path.extension().and_then(|ext| ext.to_str()) != Some("rhai")
        || !is_valid_workflow_name(stem)
    {
        return Err(ResolveError::InvalidFilename(filename));
    }
    if stem != name {
        return Err(ResolveError::FilenameMismatch {
            filename,
            name: name.to_string(),
        });
    }
    Ok(())
}

pub(crate) fn validate_workflow_name(name: &str) -> Result<(), ResolveError> {
    if is_valid_workflow_name(name) {
        Ok(())
    } else {
        Err(ResolveError::InvalidName(name.to_string()))
    }
}

fn is_valid_workflow_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_WORKFLOW_NAME_BYTES
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

pub(crate) fn read_trusted_source(path: &Path) -> Result<String, ResolveError> {
    let path_display = path.display().to_string();
    let meta = std::fs::symlink_metadata(path).map_err(|error| ResolveError::Io {
        path: path_display.clone(),
        error: error.to_string(),
    })?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ResolveError::UntrustedPath {
            path: path_display,
            reason: "expected a non-symlink regular file".into(),
        });
    }
    if meta.len() > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(ResolveError::SourceTooLarge {
            path: path_display,
            limit: MAX_WORKFLOW_SOURCE_BYTES,
        });
    }

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path).map_err(|error| ResolveError::Io {
        path: path.display().to_string(),
        error: error.to_string(),
    })?;
    let opened_meta = file.metadata().map_err(|error| ResolveError::Io {
        path: path.display().to_string(),
        error: error.to_string(),
    })?;
    if !opened_meta.is_file() || opened_meta.len() > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(if opened_meta.len() > MAX_WORKFLOW_SOURCE_BYTES {
            ResolveError::SourceTooLarge {
                path: path.display().to_string(),
                limit: MAX_WORKFLOW_SOURCE_BYTES,
            }
        } else {
            ResolveError::UntrustedPath {
                path: path.display().to_string(),
                reason: "expected a regular file".into(),
            }
        });
    }

    let mut bytes = Vec::with_capacity(opened_meta.len() as usize);
    file.take(MAX_WORKFLOW_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ResolveError::Io {
            path: path.display().to_string(),
            error: error.to_string(),
        })?;
    if bytes.len() as u64 > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(ResolveError::SourceTooLarge {
            path: path.display().to_string(),
            limit: MAX_WORKFLOW_SOURCE_BYTES,
        });
    }
    String::from_utf8(bytes).map_err(|error| ResolveError::Io {
        path: path.display().to_string(),
        error: error.to_string(),
    })
}

pub(crate) fn publish_workflow(
    session_cwd: &Path,
    scope: WorkflowScope,
    requested_name: &str,
    script: &str,
    expected_base_hash: Option<&str>,
) -> Result<PathBuf, ResolveError> {
    validate_workflow_name(requested_name)?;
    if !matches!(scope, WorkflowScope::Project | WorkflowScope::User) {
        return Err(ResolveError::PublishConflict {
            path: scope.as_str().into(),
            reason: "only project and user scopes are publishable".into(),
        });
    }
    if scope == WorkflowScope::Project
        && !crate::agent::folder_trust::project_scope_allowed(session_cwd)
    {
        return Err(ResolveError::UntrustedPath {
            path: project_root(session_cwd).display().to_string(),
            reason: "project workflows require folder trust".into(),
        });
    }
    if script.len() as u64 > MAX_WORKFLOW_SOURCE_BYTES {
        return Err(ResolveError::SourceTooLarge {
            path: "<saved workflow>".into(),
            limit: MAX_WORKFLOW_SOURCE_BYTES,
        });
    }
    let meta = parse_workflow(script, None)?;
    if meta.name != requested_name {
        return Err(ResolveError::FilenameMismatch {
            filename: format!("{requested_name}.rhai"),
            name: meta.name,
        });
    }

    let canonical_target = publish_target_path(session_cwd, scope, requested_name)?;
    match expected_base_hash {
        Some(expected) => {
            let current = read_trusted_source(&canonical_target).map_err(|error| {
                ResolveError::PublishConflict {
                    path: canonical_target.display().to_string(),
                    reason: error.to_string(),
                }
            })?;
            let actual = content_hash(&current);
            if actual != expected {
                return Err(ResolveError::PublishConflict {
                    path: canonical_target.display().to_string(),
                    reason: "the saved Definition changed after this draft was derived".into(),
                });
            }
            atomic_replace(&canonical_target, script.as_bytes()).map_err(|error| {
                ResolveError::Io {
                    path: canonical_target.display().to_string(),
                    error: error.to_string(),
                }
            })?;
        }
        None => atomic_create_new(&canonical_target, script.as_bytes()).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                ResolveError::PublishConflict {
                    path: canonical_target.display().to_string(),
                    reason: "a Definition with this name already exists in the selected scope"
                        .into(),
                }
            } else {
                ResolveError::Io {
                    path: canonical_target.display().to_string(),
                    error: error.to_string(),
                }
            }
        })?,
    }
    Ok(canonical_target)
}

/// Resolve and prepare the exact trusted publish target without writing the
/// Definition. The Workspace records this path in its write-ahead intent, and
/// [`publish_workflow`] uses the same resolver for the atomic commit.
pub(crate) fn publish_target_path(
    session_cwd: &Path,
    scope: WorkflowScope,
    requested_name: &str,
) -> Result<PathBuf, ResolveError> {
    validate_workflow_name(requested_name)?;
    if !matches!(scope, WorkflowScope::Project | WorkflowScope::User) {
        return Err(ResolveError::PublishConflict {
            path: scope.as_str().into(),
            reason: "only project and user scopes are publishable".into(),
        });
    }
    if scope == WorkflowScope::Project
        && !crate::agent::folder_trust::project_scope_allowed(session_cwd)
    {
        return Err(ResolveError::UntrustedPath {
            path: project_root(session_cwd).display().to_string(),
            reason: "project workflows require folder trust".into(),
        });
    }

    let root = match scope {
        WorkflowScope::Project => project_root(session_cwd),
        WorkflowScope::User => crate::util::grow_home::grow_home(),
        WorkflowScope::Session | WorkflowScope::Builtin => unreachable!(),
    };
    if scope == WorkflowScope::User && !root.exists() {
        std::fs::create_dir_all(&root).map_err(|error| ResolveError::Io {
            path: root.display().to_string(),
            error: error.to_string(),
        })?;
    }
    let canonical_root = dunce::canonicalize(&root).map_err(|error| ResolveError::Io {
        path: root.display().to_string(),
        error: error.to_string(),
    })?;
    let dir = match scope {
        WorkflowScope::Project => canonical_root.join(".grow").join("workflows"),
        WorkflowScope::User => canonical_root.join("workflows"),
        WorkflowScope::Session | WorkflowScope::Builtin => unreachable!(),
    };
    create_contained_workflow_dir(&canonical_root, &dir)?;
    let canonical_dir = dunce::canonicalize(&dir).map_err(|error| ResolveError::Io {
        path: dir.display().to_string(),
        error: error.to_string(),
    })?;
    let canonical_target = canonical_dir.join(format!("{requested_name}.rhai"));
    Ok(canonical_target)
}

/// Re-resolve a persisted publish target through the current trusted roots.
/// Recovery never trusts an absolute path copied from session state by itself.
pub(crate) fn validate_publish_target_path(
    session_cwd: &Path,
    scope: WorkflowScope,
    requested_name: &str,
    persisted_target: &Path,
) -> Result<PathBuf, ResolveError> {
    let expected = publish_target_path(session_cwd, scope, requested_name)?;
    if expected != persisted_target {
        return Err(ResolveError::UntrustedPath {
            path: persisted_target.display().to_string(),
            reason: "persisted publish target no longer matches the selected scope".into(),
        });
    }
    Ok(expected)
}

fn create_contained_workflow_dir(root: &Path, dir: &Path) -> Result<(), ResolveError> {
    let root = dunce::canonicalize(root).map_err(|error| ResolveError::Io {
        path: root.display().to_string(),
        error: error.to_string(),
    })?;
    let relative = dir
        .strip_prefix(&root)
        .map_err(|_| ResolveError::UntrustedPath {
            path: dir.display().to_string(),
            reason: "save directory escaped project root".into(),
        })?;

    let mut current = root.clone();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(ResolveError::UntrustedPath {
                path: dir.display().to_string(),
                reason: "save directory contains a non-normal component".into(),
            });
        };
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(ResolveError::UntrustedPath {
                    path: current.display().to_string(),
                    reason: "save directory component is not a real directory".into(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                std::fs::create_dir(&current).map_err(|error| ResolveError::Io {
                    path: current.display().to_string(),
                    error: error.to_string(),
                })?;
            }
            Err(error) => {
                return Err(ResolveError::Io {
                    path: current.display().to_string(),
                    error: error.to_string(),
                });
            }
        }
    }

    let canonical = dunce::canonicalize(dir).map_err(|error| ResolveError::Io {
        path: dir.display().to_string(),
        error: error.to_string(),
    })?;
    if !canonical.starts_with(&root) {
        return Err(ResolveError::UntrustedPath {
            path: dir.display().to_string(),
            reason: "save directory escaped project root".into(),
        });
    }
    Ok(())
}

fn atomic_create_new(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
    let temp = parent.join(format!(".workflow-{}.tmp", uuid::Uuid::now_v7().simple()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        #[cfg(unix)]
        std::fs::hard_link(&temp, target)?;
        #[cfg(windows)]
        atomic_rename_noreplace_windows(&temp, target)?;
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

fn atomic_replace(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
    let temp = parent.join(format!(".workflow-{}.tmp", uuid::Uuid::now_v7().simple()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, target)?;
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
        Ok(())
    })();
    let _ = std::fs::remove_file(&temp);
    result
}

#[cfg(windows)]
fn atomic_rename_noreplace_windows(source: &Path, target: &Path) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination already exists",
        ));
    }
    std::fs::rename(source, target)
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct WorkflowListing {
    pub definition_id: WorkflowDefinitionId,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_to_use: Option<String>,
    pub source: &'static str,
    pub scope: WorkflowScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub status: String,
    pub content_hash: String,
    pub focused: bool,
}

pub(crate) fn list_workflows(session_cwd: Option<&Path>) -> Vec<WorkflowListing> {
    WorkflowRegistry::scan(session_cwd).list()
}

pub(crate) fn workflow_snapshot(
    session_cwd: Option<&Path>,
) -> (WorkflowRegistry, Vec<WorkflowListing>) {
    let registry = WorkflowRegistry::scan(session_cwd);
    let listings = registry.list();
    (registry, listings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(name: &str) -> String {
        format!("let meta = #{{ name: \"{name}\", description: \"d\" }};\ncomplete(\"ok\");")
    }

    #[test]
    fn inline_resolution_validates_name_and_size() {
        let ok = resolve_inline(script("valid-name"));
        assert_eq!(ok.unwrap().meta.name, "valid-name");

        assert!(matches!(
            resolve_inline(script("../../escape")),
            Err(ResolveError::Meta(workflow::MetaError::InvalidName))
        ));
        assert!(matches!(
            resolve_inline("x".repeat(MAX_WORKFLOW_SOURCE_BYTES as usize + 1)),
            Err(ResolveError::SourceTooLarge { .. })
        ));
    }

    #[test]
    fn deterministic_scan_uses_git_root_and_skips_invalid_filename() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let cwd = dir.path().join("nested");
        let wf_dir = dir.path().join(".grow").join("workflows");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&wf_dir).unwrap();
        std::fs::write(wf_dir.join("alpha.rhai"), script("alpha")).unwrap();
        std::fs::write(wf_dir.join("wrong.rhai"), script("other")).unwrap();

        let registry = WorkflowRegistry::scan(Some(&cwd));
        let project_names: Vec<_> = registry
            .list()
            .into_iter()
            .filter(|entry| entry.source == "project")
            .map(|entry| entry.name)
            .collect();
        assert_eq!(project_names, ["alpha"]);
        assert_eq!(
            registry.resolve_by_name("alpha").unwrap().meta.name,
            "alpha"
        );
        assert!(matches!(
            resolve_by_path(&wf_dir.join("wrong.rhai"), &cwd, None),
            Err(ResolveError::FilenameMismatch { .. })
        ));
        assert!(registry.diagnostics().iter().any(|diagnostic| {
            diagnostic.code == "filename_mismatch"
                && diagnostic
                    .path
                    .as_deref()
                    .is_some_and(|path| path.ends_with("wrong.rhai"))
        }));
    }

    #[test]
    fn session_run_projection_resolves_despite_generic_filename() {
        let cwd_dir = tempfile::tempdir().unwrap();
        git2::Repository::init(cwd_dir.path()).unwrap();
        let session_dir = tempfile::tempdir().unwrap();
        let run_dir = session_dir.path().join("workflows/wf_1");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(run_dir.join("script.rhai"), script("alpha")).unwrap();
        let resolved = resolve_by_path(
            &run_dir.join("script.rhai"),
            cwd_dir.path(),
            Some(session_dir.path()),
        )
        .unwrap();
        assert_eq!(resolved.meta.name, "alpha");
    }

    #[test]
    fn project_workflows_follow_folder_trust() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let workflows = dir.path().join(".grow/workflows");
        std::fs::create_dir_all(&workflows).unwrap();
        std::fs::write(workflows.join("project-only.rhai"), script("project-only")).unwrap();

        crate::agent::folder_trust::record_for_test(dir.path(), false);
        let untrusted = WorkflowRegistry::scan(Some(dir.path()));
        assert!(
            untrusted
                .list()
                .iter()
                .all(|listing| listing.name != "project-only")
        );
        assert!(matches!(
            untrusted.resolve_by_name("project-only"),
            Err(ResolveError::UnknownName(_))
        ));

        crate::agent::folder_trust::record_for_test(dir.path(), true);
        let trusted = WorkflowRegistry::scan(Some(dir.path()));
        assert_eq!(
            trusted.resolve_by_name("project-only").unwrap().meta.name,
            "project-only"
        );
    }

    #[test]
    fn same_name_definitions_in_different_scopes_keep_stable_ids() {
        let entries = vec![
            RegistryEntry {
                definition_id: definition_id(WorkflowScope::Project, "same"),
                scope: WorkflowScope::Project,
                meta: extract_meta(&script("same")).unwrap(),
                script: script("same"),
                source: WorkflowSource::File(PathBuf::from("project/same.rhai")),
                source_label: "project",
                path: Some(PathBuf::from("project/same.rhai")),
            },
            RegistryEntry {
                definition_id: definition_id(WorkflowScope::Builtin, "same"),
                scope: WorkflowScope::Builtin,
                meta: extract_meta(&script("same")).unwrap(),
                script: script("same"),
                source: WorkflowSource::Builtin,
                source_label: "builtin",
                path: None,
            },
        ];
        let registry = WorkflowRegistry {
            entries,
            duplicate_names: BTreeMap::new(),
            diagnostics: Vec::new(),
        };
        assert_eq!(registry.list().len(), 2);
        assert_eq!(registry.list()[0].source, "project");
        assert_eq!(
            registry.resolve_by_name("same").unwrap().source,
            WorkflowSource::File(PathBuf::from("project/same.rhai"))
        );
    }

    #[test]
    fn duplicate_same_scope_names_are_rejected() {
        let mut entries = vec![
            RegistryEntry {
                definition_id: definition_id(WorkflowScope::Project, "dup"),
                scope: WorkflowScope::Project,
                meta: extract_meta(&script("dup")).unwrap(),
                script: script("dup"),
                source: WorkflowSource::File(PathBuf::from("a/dup.rhai")),
                source_label: "project",
                path: Some(PathBuf::from("a/dup.rhai")),
            },
            RegistryEntry {
                definition_id: definition_id(WorkflowScope::Project, "dup"),
                scope: WorkflowScope::Project,
                meta: extract_meta(&script("dup")).unwrap(),
                script: script("dup"),
                source: WorkflowSource::File(PathBuf::from("b/dup.rhai")),
                source_label: "project",
                path: Some(PathBuf::from("b/dup.rhai")),
            },
        ];
        let mut duplicate_names = BTreeMap::new();
        let mut diagnostics = Vec::new();
        reject_same_scope_duplicates(
            &mut entries,
            "project",
            WorkflowScope::Project,
            &mut duplicate_names,
            &mut diagnostics,
        );
        let registry = WorkflowRegistry {
            entries,
            duplicate_names,
            diagnostics,
        };
        assert!(matches!(
            registry.resolve_by_name("dup"),
            Err(ResolveError::DuplicateName { .. })
        ));
    }

    #[test]
    fn explicit_paths_are_relative_to_session_cwd_and_contained() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let outside = dir.path().join("outside.rhai");
        std::fs::create_dir_all(project.join("defs")).unwrap();
        std::fs::write(project.join("defs/demo.rhai"), script("demo")).unwrap();
        std::fs::write(&outside, script("outside")).unwrap();

        let resolved = resolve_by_path(Path::new("defs/demo.rhai"), &project, None).unwrap();
        assert_eq!(resolved.meta.name, "demo");
        assert!(matches!(
            resolve_by_path(&outside, &project, None),
            Err(ResolveError::UntrustedPath { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn discovery_and_explicit_paths_reject_symlink_files() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let workflows = project.join(".grow/workflows");
        let target = dir.path().join("linked.rhai");
        std::fs::create_dir_all(&workflows).unwrap();
        std::fs::write(&target, script("linked")).unwrap();
        symlink(&target, workflows.join("linked.rhai")).unwrap();

        assert!(
            WorkflowRegistry::scan(Some(&project))
                .list()
                .iter()
                .all(|entry| entry.name != "linked")
        );
        assert!(matches!(
            resolve_by_path(&workflows.join("linked.rhai"), &project, None),
            Err(ResolveError::UntrustedPath { .. })
        ));
    }

    #[test]
    fn publish_is_validated_atomic_and_no_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let path = publish_workflow(
            dir.path(),
            WorkflowScope::Project,
            "saved",
            &script("saved"),
            None,
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), script("saved"));

        let error = publish_workflow(
            dir.path(),
            WorkflowScope::Project,
            "saved",
            &script("saved"),
            None,
        )
        .unwrap_err();
        assert!(matches!(error, ResolveError::PublishConflict { .. }));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), script("saved"));
        assert!(matches!(
            publish_workflow(
                dir.path(),
                WorkflowScope::Project,
                "filename",
                &script("different"),
                None,
            ),
            Err(ResolveError::FilenameMismatch { .. })
        ));
    }

    #[cfg(unix)]
    #[test]
    fn save_through_symlinked_session_root_stays_in_canonical_project() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let linked = dir.path().join("linked-project");
        std::fs::create_dir_all(&project).unwrap();
        symlink(&project, &linked).unwrap();
        let path = publish_workflow(
            &linked,
            WorkflowScope::Project,
            "safe",
            &script("safe"),
            None,
        )
        .unwrap();
        assert_eq!(
            dunce::canonicalize(path).unwrap(),
            dunce::canonicalize(project.join(".grow/workflows/safe.rhai")).unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_rejects_symlinked_registry_directory() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let attacker = dir.path().join("attacker");
        std::fs::create_dir_all(project.join(".grow")).unwrap();
        std::fs::create_dir_all(&attacker).unwrap();
        symlink(&attacker, project.join(".grow/workflows")).unwrap();

        assert!(matches!(
            publish_workflow(
                &project,
                WorkflowScope::Project,
                "safe",
                &script("safe"),
                None,
            ),
            Err(ResolveError::UntrustedPath { .. })
        ));
        assert!(!attacker.join("safe.rhai").exists());
    }
}
