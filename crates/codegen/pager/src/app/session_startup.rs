//! Canonical session-selection CLI intent.
//!
//! Built once from CLI flags and consumed by interactive resolve, the event
//! loop, and headless mode so resume / new-with-id / fork are not re-derived
//! in three places.
use super::cli::PagerArgs;
use std::path::{Path, PathBuf};
/// Session-create intent deferred until [`AppView::session_startup_allowed`].
///
/// Replaces the prior matrix of `startup_load_session` + cwd + `startup_fork`
/// tuple + ad-hoc preferred-only replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferredSessionStartup {
    /// Strict resume (`-r` / `-c` / picker load).
    Load {
        session_id: String,
        session_cwd: Option<PathBuf>,
    },
    /// Client-chosen id (`--session-id`); also stashes preferred for picker.
    NewWithId { session_id: String },
    /// Startup `--fork-session` after parent resolve.
    Fork {
        parent_session_id: String,
        parent_cwd: Option<PathBuf>,
        new_session_id: Option<String>,
    },
}
/// One owner for every action deferred behind auth/folder-trust startup gates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeferredStartupActions {
    pub session: Option<DeferredSessionStartup>,
    pub preferred_session_id: Option<String>,
    pub worktree: bool,
    pub worktree_label: Option<String>,
    pub worktree_ref: Option<String>,
    pub new_session: bool,
    pub prompt: Option<String>,
    pub open_dashboard: bool,
}
impl DeferredStartupActions {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }
}
/// Build `grow/session/fork` params shared by TUI effects and headless.
///
/// `new_cwd` is the write namespace for the child (parent session cwd when
/// cross-cwd); preflight must use the same path via [`effective_fork_new_cwd`].
pub fn fork_session_params(
    parent_session_id: &str,
    parent_cwd: &Path,
    new_session_id: Option<&str>,
    parent_is_worktree: bool,
) -> serde_json::Value {
    let parent_cwd_str = parent_cwd.to_string_lossy().into_owned();
    let source_cwd = shell::session::resolve_local_session_any_cwd(parent_session_id)
        .unwrap_or_else(|| parent_cwd_str.clone());
    let mut payload = serde_json::json!({
        "sourceSessionId": parent_session_id,
        "sourceCwd": source_cwd,
        "newCwd": parent_cwd_str.clone(),
        "sessionKind": "fork",
    });
    if let Some(nid) = new_session_id {
        payload["newSessionId"] = serde_json::Value::String(nid.to_string());
    }
    if parent_is_worktree {
        payload["sourceWorkspaceDir"] = serde_json::Value::String(parent_cwd_str);
    }
    payload
}
/// Whether a persisted session (or its cwd) is worktree-backed.
/// Mirrors in-session `/fork` reading `agent.session.is_worktree`.
pub fn parent_session_is_worktree(session_id: &str, cwd: &Path) -> bool {
    let cwd_str = cwd.to_string_lossy();
    let sessions_root = shell::util::grow_home::grow_home().join("sessions");
    let encoded = shell::util::grow_home::encode_cwd_dirname(&cwd_str);
    let summary_path = sessions_root
        .join(encoded)
        .join(session_id)
        .join("summary.json");
    if let Ok(bytes) = std::fs::read(&summary_path)
        && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        if v.get("session_kind").and_then(|k| k.as_str()) == Some("worktree") {
            return true;
        }
        if v.get("source_workspace_dir")
            .and_then(|k| k.as_str())
            .is_some_and(|s| !s.is_empty())
        {
            return true;
        }
    }
    let mut cur = Some(cwd);
    while let Some(dir) = cur {
        let git = dir.join(".git");
        if git.is_file() {
            return true;
        }
        if git.is_dir() {
            return false;
        }
        cur = dir.parent();
    }
    false
}
/// Parse `newSessionId` from an `grow/session/fork` ACP response body.
pub fn fork_response_new_session_id(resp_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(resp_json).unwrap_or_default();
    if v.get("error").is_some_and(|e| !e.is_null()) {
        return None;
    }
    v.get("newSessionId")
        .and_then(|x| x.as_str())
        .or_else(|| {
            v.get("result")
                .and_then(|r| r.get("newSessionId"))
                .and_then(|x| x.as_str())
        })
        .map(|s| s.to_string())
}
/// Error string from a fork response, if present.
pub fn fork_response_error(resp_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(resp_json).ok()?;
    v.get("error")
        .filter(|e| !e.is_null())
        .map(|e| e.to_string())
}
/// Pure interpretation of session-selection CLI flags (no I/O).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStartupIntent {
    /// Fresh session; agent picks the ID.
    NewAuto,
    /// Fresh session with a client-chosen ID (must not exist under cwd).
    NewWithId { session_id: String },
    /// Load an existing session (strict — never create).
    Resume {
        /// `None` means resolve most-recent for cwd at materialize time.
        session_id: Option<String>,
        most_recent_for_cwd: bool,
    },
    /// Resolve source like resume, then fork; optional forced ID for the child.
    ForkFrom {
        source_session_id: Option<String>,
        most_recent_for_cwd: bool,
        new_session_id: Option<String>,
    },
}
/// Flag combinations that clap allows but we reject at runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupFlagError {
    /// `--session-id` with resume/continue/load without `--fork-session`.
    SessionIdRequiresFork,
    /// `--fork-session` without resume/continue/load.
    ForkRequiresResumeOrContinue,
    /// `--fork-session` with `--worktree` (not supported yet).
    ForkWithWorktree,
}
impl std::fmt::Display for StartupFlagError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SessionIdRequiresFork => {
                write!(
                    f,
                    "Error: --session-id can only be used with --continue or --resume if --fork-session is also specified."
                )
            }
            Self::ForkRequiresResumeOrContinue => {
                write!(f, "Error: --fork-session requires --resume or --continue.")
            }
            Self::ForkWithWorktree => {
                write!(
                    f,
                    "Error: --fork-session cannot be combined with --worktree."
                )
            }
        }
    }
}
impl std::error::Error for StartupFlagError {}
/// Inputs shared by interactive CLI and headless (no clap dependency).
#[derive(Debug, Clone, Copy)]
pub struct SessionStartupFlags<'a> {
    pub session_id: Option<&'a str>,
    /// Explicit resume id from `-r` / `--resume` (not the empty most-recent sentinel).
    pub resume_session_id: Option<&'a str>,
    /// `--resume` with no value (most recent for cwd).
    pub resume_most_recent: bool,
    pub continue_last_session: bool,
    pub fork_session: bool,
    /// True when `--worktree` is set (any label, including empty default).
    pub has_worktree: bool,
}
/// Classify session-selection flags into a single intent (no I/O).
pub fn session_startup_intent_from_flags(
    f: SessionStartupFlags<'_>,
) -> Result<SessionStartupIntent, StartupFlagError> {
    let has_resume_id = f.resume_session_id.is_some();
    let most_recent = f.resume_most_recent || f.continue_last_session;
    let has_resume_or_continue = has_resume_id || most_recent;
    if f.fork_session && f.has_worktree {
        return Err(StartupFlagError::ForkWithWorktree);
    }
    if f.fork_session && !has_resume_or_continue {
        return Err(StartupFlagError::ForkRequiresResumeOrContinue);
    }
    if let Some(sid) = f.session_id {
        if has_resume_or_continue && !f.fork_session {
            return Err(StartupFlagError::SessionIdRequiresFork);
        }
        if f.fork_session {
            return Ok(SessionStartupIntent::ForkFrom {
                source_session_id: f.resume_session_id.map(|s| s.to_owned()),
                most_recent_for_cwd: most_recent && !has_resume_id,
                new_session_id: Some(sid.to_owned()),
            });
        }
        return Ok(SessionStartupIntent::NewWithId {
            session_id: sid.to_owned(),
        });
    }
    if f.fork_session {
        return Ok(SessionStartupIntent::ForkFrom {
            source_session_id: f.resume_session_id.map(|s| s.to_owned()),
            most_recent_for_cwd: most_recent && !has_resume_id,
            new_session_id: None,
        });
    }
    if let Some(id) = f.resume_session_id {
        return Ok(SessionStartupIntent::Resume {
            session_id: Some(id.to_owned()),
            most_recent_for_cwd: false,
        });
    }
    if most_recent {
        return Ok(SessionStartupIntent::Resume {
            session_id: None,
            most_recent_for_cwd: true,
        });
    }
    Ok(SessionStartupIntent::NewAuto)
}
impl PagerArgs {
    /// Classify session-selection flags into a single intent (no I/O).
    pub fn session_startup_intent(&self) -> Result<SessionStartupIntent, StartupFlagError> {
        session_startup_intent_from_flags(SessionStartupFlags {
            session_id: self.session_id.as_deref(),
            resume_session_id: self.session_to_resume(),
            resume_most_recent: self.resume_most_recent(),
            continue_last_session: self.continue_last_session,
            fork_session: self.fork_session,
            has_worktree: self.worktree.is_some(),
        })
    }
}
/// Outcome of async materialization (local resolve / remote restore / preflight).
#[derive(Debug, Clone)]
pub enum MaterializedStartup {
    /// Create a new session with an agent-chosen ID (or defer to welcome).
    NewAuto,
    /// Create a new session with this ID (`session/new` meta.sessionId).
    NewWithId { session_id: String },
    /// Strict load of an existing session.
    Resume {
        session_id: String,
        original_cwd: Option<PathBuf>,
        title: Option<String>,
        /// The target missed local id/title resolution and was deferred to
        /// the worktree resume handler; worktree failure messages append the
        /// no-match hint only for this outcome (never inferred from shape).
        deferred_local_miss: bool,
    },
    /// Fork from a resolved parent, then load the child.
    Fork {
        parent_session_id: String,
        parent_cwd: Option<PathBuf>,
        parent_title: Option<String>,
        new_session_id: Option<String>,
    },
}
/// Whether materialization may resolve a non-id resume arg by title locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleResolution {
    /// No pre-sandbox pin ran (direct callers, tests): materialization owns
    /// title selection.
    Allowed,
    /// The composition root already pinned — or definitively missed — the
    /// target before the irreversible OS sandbox. Re-selecting by title here
    /// would race a concurrent rename/create and resume a session whose
    /// persisted profile was never checked; a pinned id that vanished must
    /// also never be reinterpreted as a title.
    PinnedPreSandbox,
}
/// Context for [`materialize_startup`] (interactive vs headless share this).
#[derive(Debug, Clone, Copy)]
pub struct MaterializeCtx {
    /// When true, skip process-cwd preflight for `NewWithId` (worktree create
    /// checks the final session cwd later).
    pub has_worktree: bool,
    /// See [`TitleResolution`]; carried from the pre-sandbox pin outcome.
    pub title_resolution: TitleResolution,
}
impl MaterializeCtx {
    pub fn from_pager_args(args: &PagerArgs) -> Self {
        Self {
            has_worktree: args.worktree.is_some(),
            title_resolution: if args.resume_target_pinned {
                TitleResolution::PinnedPreSandbox
            } else {
                TitleResolution::Allowed
            },
        }
    }
}
/// Cwd where a forked child session is written (interactive + headless SSOT).
///
/// When the parent lives under another directory, the fork effect sets
/// `newCwd` to that parent session cwd — preflight must use the same path.
pub fn effective_fork_new_cwd(process_cwd: &str, parent_cwd: Option<&Path>) -> String {
    parent_cwd
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| process_cwd.to_string())
}
/// Resolve most-recent session id for cwd, or error.
async fn most_recent_session_id(cwd: &str) -> anyhow::Result<(String, Option<String>)> {
    let summaries = shell::session::persistence::list_summaries(Some(cwd)).await?;
    let first = summaries.first().ok_or_else(|| {
        anyhow::anyhow!(
            "No session found for current directory. \
             Use 'grow' to start a new session."
        )
    })?;
    Ok((first.info.id.to_string(), first.display_title_opt()))
}
/// Preflight: preferred id must be a UUID and not a persisted session under `cwd`.
///
/// Agent `session/new` rejects non-UUID `_meta.sessionId`; fail fast here so
/// CLI users get a clear error before ACP.
pub fn ensure_session_id_available(session_id: &str, cwd: &str) -> anyhow::Result<()> {
    if uuid::Uuid::try_parse(session_id).is_err() {
        anyhow::bail!("Error: --session-id must be a valid UUID (got '{session_id}').");
    }
    if shell::session::persistence::session_exists_for_cwd(session_id, cwd) {
        anyhow::bail!("Error: Session ID {session_id} is already in use.");
    }
    Ok(())
}
/// Materialize CLI intent into a concrete startup plan (I/O + remote restore).
pub async fn materialize_startup(
    ctx: MaterializeCtx,
    intent: SessionStartupIntent,
) -> anyhow::Result<MaterializedStartup> {
    let cwd = std::env::current_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get cwd: {e}"))?
        .to_string_lossy()
        .to_string();
    materialize_startup_for_cwd(ctx, intent, &cwd).await
}
/// Same as [`materialize_startup`] but with an explicit process cwd (tests / headless).
pub async fn materialize_startup_for_cwd(
    ctx: MaterializeCtx,
    intent: SessionStartupIntent,
    cwd: &str,
) -> anyhow::Result<MaterializedStartup> {
    match intent {
        SessionStartupIntent::NewAuto => Ok(MaterializedStartup::NewAuto),
        SessionStartupIntent::NewWithId { session_id } => {
            if !ctx.has_worktree {
                ensure_session_id_available(&session_id, cwd)?;
            } else if uuid::Uuid::try_parse(&session_id).is_err() {
                anyhow::bail!("Error: --session-id must be a valid UUID (got '{session_id}').");
            }
            Ok(MaterializedStartup::NewWithId { session_id })
        }
        SessionStartupIntent::Resume {
            session_id: None,
            most_recent_for_cwd: true,
        } => {
            let started = std::time::Instant::now();
            let (id, title) = most_recent_session_id(cwd).await?;
            tracing::info!(
                source = "local",
                elapsed_ms = started.elapsed().as_millis() as u64,
                "startup.continue.resolve"
            );
            Ok(MaterializedStartup::Resume {
                session_id: id,
                original_cwd: None,
                title,
                deferred_local_miss: false,
            })
        }
        SessionStartupIntent::ForkFrom {
            source_session_id: None,
            most_recent_for_cwd: true,
            new_session_id,
        } => {
            if let Some(ref nid) = new_session_id {
                ensure_session_id_available(nid, cwd)?;
            }
            let (id, title) = most_recent_session_id(cwd).await?;
            Ok(MaterializedStartup::Fork {
                parent_session_id: id,
                parent_cwd: None,
                parent_title: title,
                new_session_id,
            })
        }
        SessionStartupIntent::Resume {
            session_id: Some(session_id),
            ..
        } => {
            let r = resolve_existing_session(ctx, &session_id, cwd).await?;
            Ok(MaterializedStartup::Resume {
                session_id: r.id,
                original_cwd: r.original_cwd,
                title: r.title,
                deferred_local_miss: r.deferred_local_miss,
            })
        }
        SessionStartupIntent::ForkFrom {
            source_session_id: Some(session_id),
            new_session_id,
            ..
        } => {
            let r = resolve_existing_session(ctx, &session_id, cwd).await?;
            if let Some(ref nid) = new_session_id {
                let new_cwd = effective_fork_new_cwd(cwd, r.original_cwd.as_deref());
                ensure_session_id_available(nid, &new_cwd)?;
            }
            Ok(MaterializedStartup::Fork {
                parent_session_id: r.id,
                parent_cwd: r.original_cwd,
                parent_title: r.title,
                new_session_id,
            })
        }
        SessionStartupIntent::Resume {
            session_id: None,
            most_recent_for_cwd: false,
        }
        | SessionStartupIntent::ForkFrom {
            source_session_id: None,
            most_recent_for_cwd: false,
            ..
        } => {
            anyhow::bail!("internal: invalid session startup intent (unreachable from CLI flags)")
        }
    }
}
struct ResolvedExisting {
    id: String,
    original_cwd: Option<PathBuf>,
    title: Option<String>,
    /// True only for the worktree-defer arm: the target missed local
    /// id/title resolution.
    deferred_local_miss: bool,
}
/// Resolve an existing session for strict resume (local / any-cwd / remote / worktree defer).
async fn resolve_existing_session(
    ctx: MaterializeCtx,
    session_id: &str,
    cwd: &str,
) -> anyhow::Result<ResolvedExisting> {
    if let Some(local_id) = shell::session::resolve_local_session(session_id, cwd) {
        tracing::info!(session_id = %session_id, local_id = %local_id, "Session found locally");
        return Ok(ResolvedExisting {
            id: local_id,
            original_cwd: None,
            title: None,
            deferred_local_miss: false,
        });
    }
    if let Some(original_cwd) = shell::session::resolve_local_session_any_cwd(session_id) {
        tracing::info!(
            session_id = %session_id,
            original_cwd = %original_cwd,
            "Session found locally under different CWD"
        );
        eprintln!(
            "Session {} found locally (originally in {})",
            session_id, original_cwd
        );
        return Ok(ResolvedExisting {
            id: session_id.to_string(),
            original_cwd: Some(PathBuf::from(original_cwd)),
            title: None,
            deferred_local_miss: false,
        });
    }
    let arg_is_uuid = super::session_title_resolve::is_uuid_shaped(session_id);
    if !arg_is_uuid
        && ctx.title_resolution == TitleResolution::Allowed
        && let Some(resolved) = resolve_session_by_title(session_id, cwd).await?
    {
        return Ok(resolved);
    }
    if ctx.has_worktree {
        tracing::info!(
            session_id = %session_id,
            "Session not found locally; deferring restore to worktree resume handler"
        );
        eprintln!(
            "Session {:?} not found locally; it will be restored into the new worktree.",
            session_id
        );
        return Ok(ResolvedExisting {
            id: session_id.to_string(),
            original_cwd: None,
            title: None,
            deferred_local_miss: !arg_is_uuid,
        });
    }
    if !arg_is_uuid {
        anyhow::bail!(
            "Session does not exist: {}",
            super::session_title_resolve::title_miss_hint(session_id)
        );
    } else {
        anyhow::bail!("Session does not exist")
    }
}
/// Resolve a non-id resume arg as a session title among local sessions for `cwd`.
///
/// Matching/disambiguation rules live in [`super::session_title_resolve`]
/// (shared with the pre-sandbox saved-profile peek); this adds the cwd-scoped
/// listing and the resolved-id announcement. The arg is matched in memory and
/// never used as a filesystem path.
async fn resolve_session_by_title(
    arg: &str,
    cwd: &str,
) -> anyhow::Result<Option<ResolvedExisting>> {
    let summaries = shell::session::persistence::list_summaries(Some(cwd)).await?;
    let Some(chosen) = super::session_title_resolve::select_by_title(arg, &summaries)? else {
        return Ok(None);
    };
    let id = chosen.info.id.to_string();
    tracing::info!(session_id = %id, "Session resolved by title");
    eprintln!("Resuming session {} (matched by title)", id);
    Ok(Some(ResolvedExisting {
        id,
        original_cwd: None,
        title: chosen.display_title_opt(),
        deferred_local_miss: false,
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    fn parse(args: &[&str]) -> PagerArgs {
        PagerArgs::try_parse_from(args).unwrap()
    }
    #[test]
    fn deferred_startup_owner_take_is_atomic() {
        let mut actions = DeferredStartupActions {
            session: Some(DeferredSessionStartup::NewWithId {
                session_id: "session-id".into(),
            }),
            prompt: Some("prompt".into()),
            ..Default::default()
        };
        assert!(!actions.is_empty());
        let snapshot = actions.take();
        assert!(actions.is_empty());
        assert!(snapshot.session.is_some());
        assert_eq!(snapshot.prompt.as_deref(), Some("prompt"));
    }
    #[test]
    fn intent_default_is_new_auto() {
        assert_eq!(
            parse(&["grow"]).session_startup_intent().unwrap(),
            SessionStartupIntent::NewAuto
        );
    }
    #[test]
    fn intent_resume_id() {
        assert_eq!(
            parse(&["grow", "--resume", "abc"])
                .session_startup_intent()
                .unwrap(),
            SessionStartupIntent::Resume {
                session_id: Some("abc".into()),
                most_recent_for_cwd: false,
            }
        );
    }
    #[test]
    fn intent_resume_empty_is_most_recent() {
        assert_eq!(
            parse(&["grow", "--resume"])
                .session_startup_intent()
                .unwrap(),
            SessionStartupIntent::Resume {
                session_id: None,
                most_recent_for_cwd: true,
            }
        );
    }
    #[test]
    fn intent_continue() {
        assert_eq!(
            parse(&["grow", "-c"]).session_startup_intent().unwrap(),
            SessionStartupIntent::Resume {
                session_id: None,
                most_recent_for_cwd: true,
            }
        );
    }
    #[test]
    fn intent_session_id_alone_is_new_with_id() {
        assert_eq!(
            parse(&["grow", "--session-id", "my-id"])
                .session_startup_intent()
                .unwrap(),
            SessionStartupIntent::NewWithId {
                session_id: "my-id".into(),
            }
        );
    }
    #[test]
    fn intent_session_id_with_resume_without_fork_errors() {
        let err = parse(&["grow", "-r", "a", "-s", "b"])
            .session_startup_intent()
            .unwrap_err();
        assert_eq!(err, StartupFlagError::SessionIdRequiresFork);
    }
    #[test]
    fn intent_fork_with_resume() {
        assert_eq!(
            parse(&["grow", "-r", "old", "--fork-session"])
                .session_startup_intent()
                .unwrap(),
            SessionStartupIntent::ForkFrom {
                source_session_id: Some("old".into()),
                most_recent_for_cwd: false,
                new_session_id: None,
            }
        );
    }
    #[test]
    fn intent_fork_with_resume_and_new_id() {
        assert_eq!(
            parse(&["grow", "-r", "old", "--fork-session", "-s", "new"])
                .session_startup_intent()
                .unwrap(),
            SessionStartupIntent::ForkFrom {
                source_session_id: Some("old".into()),
                most_recent_for_cwd: false,
                new_session_id: Some("new".into()),
            }
        );
    }
    #[test]
    fn intent_fork_alone_errors() {
        let err = parse(&["grow", "--fork-session"])
            .session_startup_intent()
            .unwrap_err();
        assert_eq!(err, StartupFlagError::ForkRequiresResumeOrContinue);
    }
    #[test]
    fn intent_fork_with_worktree_errors() {
        let err = parse(&["grow", "-r", "a", "--fork-session", "-w"])
            .session_startup_intent()
            .unwrap_err();
        assert_eq!(err, StartupFlagError::ForkWithWorktree);
    }
    #[test]
    fn intent_from_flags_matches_pager_args() {
        let args = parse(&["grow", "-r", "old", "--fork-session", "-s", "new"]);
        let from_flags = session_startup_intent_from_flags(SessionStartupFlags {
            session_id: Some("new"),
            resume_session_id: Some("old"),
            resume_most_recent: false,
            continue_last_session: false,
            fork_session: true,
            has_worktree: false,
        })
        .unwrap();
        assert_eq!(from_flags, args.session_startup_intent().unwrap());
    }
    #[test]
    fn ensure_rejects_non_uuid() {
        let err = ensure_session_id_available("my-run-1", "/tmp/does-not-matter").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("must be a valid UUID"),
            "unexpected message: {msg}"
        );
    }
    #[test]
    fn effective_fork_new_cwd_prefers_parent() {
        let parent = PathBuf::from("/proj-a");
        assert_eq!(
            effective_fork_new_cwd("/proj-b", Some(parent.as_path())),
            "/proj-a"
        );
        assert_eq!(effective_fork_new_cwd("/proj-b", None), "/proj-b");
    }
    #[test]
    fn fork_session_params_sets_new_session_id_and_workspace_dir() {
        let cwd = PathBuf::from("/wt");
        let p = fork_session_params("parent-1", &cwd, Some("child-uuid"), true);
        assert_eq!(p["sourceSessionId"], "parent-1");
        assert_eq!(p["newCwd"], "/wt");
        assert_eq!(p["newSessionId"], "child-uuid");
        assert_eq!(p["sourceWorkspaceDir"], "/wt");
        assert_eq!(p["sessionKind"], "fork");
    }
    #[test]
    fn fork_session_params_omits_workspace_dir_when_not_worktree() {
        let cwd = PathBuf::from("/proj");
        let p = fork_session_params("parent-1", &cwd, None, false);
        assert!(p.get("sourceWorkspaceDir").is_none());
        assert!(p.get("newSessionId").is_none());
    }
    #[test]
    fn fork_response_parses_nested_and_top_level_id() {
        assert_eq!(
            fork_response_new_session_id(r#"{"newSessionId":"a"}"#).as_deref(),
            Some("a")
        );
        assert_eq!(
            fork_response_new_session_id(r#"{"result":{"newSessionId":"b"}}"#).as_deref(),
            Some("b")
        );
        assert!(fork_response_new_session_id(r#"{"error":"nope"}"#).is_none());
        assert_eq!(
            fork_response_error(r#"{"error":"boom"}"#).as_deref(),
            Some("\"boom\"")
        );
    }
    #[test]
    fn deferred_session_intent_variants_are_distinct() {
        let load = DeferredSessionStartup::Load {
            session_id: "s".into(),
            session_cwd: None,
        };
        let nid = DeferredSessionStartup::NewWithId {
            session_id: "s".into(),
        };
        assert_ne!(load, nid);
    }
    mod resume_by_title {
        use super::*;
        use crate::test_util::GrowHomeFixture;
        fn local_ctx() -> MaterializeCtx {
            MaterializeCtx {
                has_worktree: false,
                title_resolution: TitleResolution::Allowed,
            }
        }
        async fn resume(arg: &str, cwd: &str) -> anyhow::Result<MaterializedStartup> {
            materialize_startup_for_cwd(
                local_ctx(),
                SessionStartupIntent::Resume {
                    session_id: Some(arg.into()),
                    most_recent_for_cwd: false,
                },
                cwd,
            )
            .await
        }
        /// Also covers letter-case insensitivity: the query case differs from
        /// the stored title.
        #[serial_test::serial(GROW_HOME)]
        #[tokio::test]
        async fn title_fallback_resumes_single_match_case_insensitively() {
            let mut fx = GrowHomeFixture::new();
            let cwd_str = fx.cwd_str();
            let id = "bbbbbbbb-1111-2222-3333-444444444444";
            fx.write_summary(
                &cwd_str,
                id,
                serde_json::json!({
                    "title": "Fix Login Bug",
                    "title_source": { "kind": "user" },
                    "title_event_seq": 1,
                }),
            );
            fx.write_summary(
                &cwd_str,
                "bbbbbbbb-1111-2222-3333-555555555555",
                serde_json::json!({
                    "title": "Other Work",
                    "title_source": {
                        "kind": "generated",
                        "sideband_id": "018f0000-0000-7000-8000-000000000001",
                        "result_seq": 1,
                    },
                    "title_event_seq": 1,
                }),
            );
            match resume("fix login bug", &cwd_str).await.unwrap() {
                MaterializedStartup::Resume {
                    session_id,
                    original_cwd,
                    title,
                    ..
                } => {
                    assert_eq!(session_id, id);
                    assert!(original_cwd.is_none());
                    assert_eq!(title.as_deref(), Some("Fix Login Bug"));
                }
                other => panic!("expected Resume, got {other:?}"),
            }
        }
        /// Id resolution stays authoritative: when the arg is an on-disk
        /// session id, the title fallback is never consulted even though
        /// another session carries that exact title.
        #[serial_test::serial(GROW_HOME)]
        #[tokio::test]
        async fn id_hit_beats_title_fallback() {
            let mut fx = GrowHomeFixture::new();
            let cwd_str = fx.cwd_str();
            fx.write_summary(
                &cwd_str,
                "release-notes",
                serde_json::json!({
                    "title": "id-owner",
                    "title_source": {
                        "kind": "generated",
                        "sideband_id": "018f0000-0000-7000-8000-000000000001",
                        "result_seq": 1,
                    },
                    "title_event_seq": 1,
                }),
            );
            fx.write_summary(
                &cwd_str,
                "cccccccc-1111-2222-3333-444444444444",
                serde_json::json!({
                    "title": "release-notes",
                    "title_source": { "kind": "user" },
                    "title_event_seq": 1,
                }),
            );
            match resume("release-notes", &cwd_str).await.unwrap() {
                MaterializedStartup::Resume {
                    session_id, title, ..
                } => {
                    assert_eq!(session_id, "release-notes");
                    assert!(title.is_none());
                }
                other => panic!("expected Resume, got {other:?}"),
            }
        }
        /// Provenance for the worktree failure hint: only the defer arm (a
        /// local id/title miss under `--worktree`) flags the target; a
        /// resolved local id — even a legacy non-UUID one — never does.
        #[serial_test::serial(GROW_HOME)]
        #[tokio::test]
        async fn worktree_defer_flags_local_miss_and_local_hit_does_not() {
            let mut fx = GrowHomeFixture::new();
            let cwd_str = fx.cwd_str();
            fx.write_summary(&cwd_str, "release-notes", serde_json::json!({}));
            let worktree_ctx = MaterializeCtx {
                has_worktree: true,
                ..local_ctx()
            };
            let resume_intent = |arg: &str| SessionStartupIntent::Resume {
                session_id: Some(arg.into()),
                most_recent_for_cwd: false,
            };
            let hit =
                materialize_startup_for_cwd(worktree_ctx, resume_intent("release-notes"), &cwd_str)
                    .await
                    .unwrap();
            match hit {
                MaterializedStartup::Resume {
                    session_id,
                    deferred_local_miss,
                    ..
                } => {
                    assert_eq!(session_id, "release-notes");
                    assert!(!deferred_local_miss, "resolved id must not flag a miss");
                }
                other => panic!("expected Resume, got {other:?}"),
            }
            let miss = materialize_startup_for_cwd(
                worktree_ctx,
                resume_intent("no such target"),
                &cwd_str,
            )
            .await
            .unwrap();
            match miss {
                MaterializedStartup::Resume {
                    session_id,
                    deferred_local_miss,
                    ..
                } => {
                    assert_eq!(session_id, "no such target");
                    assert!(deferred_local_miss, "defer must flag the local miss");
                }
                other => panic!("expected Resume, got {other:?}"),
            }
            let uuid_miss = materialize_startup_for_cwd(
                worktree_ctx,
                resume_intent("99999999-9999-4999-8999-999999999999"),
                &cwd_str,
            )
            .await
            .unwrap();
            match uuid_miss {
                MaterializedStartup::Resume {
                    deferred_local_miss,
                    ..
                } => {
                    assert!(
                        !deferred_local_miss,
                        "UUID defer must not flag a title-capable miss"
                    );
                }
                other => panic!("expected Resume, got {other:?}"),
            }
        }
    }
}
