//! Folder-trust DECISION side ("do you trust this folder?").
//!
//! This is the client/workspace half of the folder-trust gate: it scans a
//! workspace for repo-local code-exec configs, resolves the pure trust
//! [`decide`] precedence, prompts (MVP stderr), and reads/writes the durable
//! [`crate::trust::TrustStore`] (`~/.grow/trusted_folders.toml`). The
//! consume/gating half (the `DECISIONS` cache, `resolve_and_record`,
//! `project_scope_allowed`, the loader filters) lives in `shell`.
//!
//! ## Precedence (canonical — see [`decide`])
//! 1. Feature flag OFF  → trusted (no gating; preserves prior behavior).
//! 2. Workspace/source entity cannot be identified → untrusted (fail closed).
//! 3. Store (this exact workspace entity recorded trusted) → trusted. An
//!    explicit `--trust` grant is persisted to the store up front (see
//!    [`grant_folder_trust`]), so it is honored here.
//! 4. Key unrecordable (an over-broad root — the user's own `$HOME` / filesystem
//!    root / non-absolute — that the store refuses to persist) → trusted: it
//!    can't be durably gated, so gating would re-prompt forever on a key that can
//!    never persist. See [`crate::trust::is_unsafe_trust_root`].
//! 5. No repo-local code-exec configs present → trusted (nothing to gate).
//! 6. Interactive TTY   → prompt the user (y/N).
//! 7. Otherwise (headless) → untrusted.
//!
//! (How the consume side caches this verdict — e.g. that the rule-5 allow is
//! provisional and re-checked rather than cached — is a `shell`
//! concern, documented there.)

use std::io::IsTerminal;
use std::path::Path;

use config_types::{BoolFlag, RemoteSettings};
use toml::Value as TomlValue;

use crate::trust::{TrustStore, WorkspaceIdentity, workspace_identity_for_cwd, workspace_key};

/// The pure trust outcome for a set of inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustOutcome {
    /// Repo-local servers allowed.
    Trusted,
    /// Repo-local servers blocked.
    Untrusted,
    /// Interactive: ask the user.
    Prompt,
}

/// Inputs to the pure [`decide`] precedence function.
#[derive(Debug, Clone)]
pub struct DecideInputs {
    /// Complete identity observed before the decision/prompt. This exact value
    /// must be carried through persistence; `None` always fails closed.
    pub expected_identity: Option<WorkspaceIdentity>,
    pub store_trusted: bool,
    pub repo_configs_present: bool,
    pub is_interactive: bool,
    /// False when the workspace key is an over-broad root the store refuses to
    /// record — home / filesystem root / non-absolute; see
    /// [`crate::trust::is_unsafe_trust_root`].
    pub key_recordable: bool,
}

/// Pure trust-decision precedence. No I/O; unit-tested directly.
///
/// See the module docs for the ordered precedence.
pub fn decide(feature_enabled: bool, i: &DecideInputs) -> TrustOutcome {
    if !feature_enabled {
        return TrustOutcome::Trusted;
    }
    if i.expected_identity.is_none() {
        return TrustOutcome::Untrusted;
    }
    if i.store_trusted {
        return TrustOutcome::Trusted;
    }
    // An over-broad root the store can't record (the user's own $HOME / fs-root,
    // never a fetched repo) can't be durably gated — trust it instead of
    // prompting on a key that can never persist (mirrors the feature-off default).
    if !i.key_recordable {
        return TrustOutcome::Trusted;
    }
    if !i.repo_configs_present {
        return TrustOutcome::Trusted;
    }
    if i.is_interactive {
        return TrustOutcome::Prompt;
    }
    TrustOutcome::Untrusted
}

/// Gather the [`DecideInputs`] for `cwd` (store trust + repo configs +
/// interactivity), keyed by `key`. Single-sourced gather behind the shell's
/// `compute` and launch-dir resolve so the store read and repo-config scan
/// cannot drift across callers.
pub fn decide_inputs(cwd: &Path, key: &Path) -> DecideInputs {
    let expected_identity = workspace_identity_for_cwd(cwd, key).ok();
    gather_decide_inputs(cwd, key, is_interactive(), expected_identity)
}

/// Gather decision inputs for an identity already captured at admission.
///
/// This prevents cache lookup, prompt display, and persistence from silently
/// switching to a replacement entity through independent path re-resolution.
pub fn decide_inputs_with_expected_identity(
    cwd: &Path,
    key: &Path,
    expected_identity: Option<WorkspaceIdentity>,
) -> DecideInputs {
    gather_decide_inputs(cwd, key, is_interactive(), expected_identity)
}

/// Like [`decide_inputs`] but with caller-supplied interactivity, so the gather
/// (store trust + repo-config scan) stays single-sourced across callers that
/// determine interactivity differently. The pager TUI passes
/// `stdin().is_terminal()` ONLY: it redirects native stderr before resolving
/// trust, so the default [`is_interactive`] (`stdin && stderr`) would be false
/// and the question could never show.
pub fn decide_inputs_with_interactive(
    cwd: &Path,
    key: &Path,
    is_interactive: bool,
) -> DecideInputs {
    let expected_identity = workspace_identity_for_cwd(cwd, key).ok();
    gather_decide_inputs(cwd, key, is_interactive, expected_identity)
}

fn gather_decide_inputs(
    cwd: &Path,
    key: &Path,
    is_interactive: bool,
    expected_identity: Option<WorkspaceIdentity>,
) -> DecideInputs {
    let mut expected_identity = expected_identity.filter(|expected| {
        workspace_identity_for_cwd(cwd, key).is_ok_and(|current| current == *expected)
    });
    let mut store_trusted = expected_identity
        .as_ref()
        .is_some_and(|identity| TrustStore::load().is_trusted_identity(key, identity));
    let repo_configs_present = repo_configs_present(cwd);
    // The config scan can take long enough for a same-path checkout replacement.
    // Revalidate the same captured identity after all decision I/O; a mismatch
    // clears both identity and any store match derived from it.
    if expected_identity.as_ref().is_some_and(|expected| {
        !workspace_identity_for_cwd(cwd, key).is_ok_and(|current| current == *expected)
    }) {
        expected_identity = None;
        store_trusted = false;
    }
    DecideInputs {
        expected_identity,
        store_trusted,
        // Deliberate second discover: the caller's `key` came from `workspace_key`
        // (its own git2 discover), and `repo_configs_present` → `RepoDirChain::resolve`
        // discovers the same repo again. Collapsing the two would mean threading the
        // resolved root into key derivation (rippling `workspace_key` repo-wide) — out
        // of scope; NOT the redundant discovers this change already removed.
        repo_configs_present,
        is_interactive,
        // An over-broad key (home / fs-root / non-absolute) can never be recorded
        // by the store, so decide() trusts it rather than prompt on a key that
        // can't persist (Case 2: cwd IS $HOME, incl. the default `~/.grow`).
        key_recordable: !crate::trust::is_unsafe_trust_root(key),
    }
}

/// Whether the whole folder-trust system is inert (auto-trusts everything) for
/// this binary — true on a local/dev build (no `GROW_VERSION` release stamp).
///
/// THE single security short-circuit: every explicit trust auto-grant site calls
/// this (greppable via `folder_trust_inert`). When true a self-built grow never
/// prompts, never gates repo-local `.envrc`/hooks/plugins/MCP/LSP, and
/// does NO `trusted_folders.toml` I/O. Release-stamped builds are unaffected.
pub fn folder_trust_inert() -> bool {
    is_local_build()
}

/// Whether this binary was built without a release version stamp
/// (`GROW_VERSION` unset at compile time) — i.e. a local/dev build.
///
/// Kept local (not in `version`) on purpose: adding a symbol to that
/// near-universal crate widens the rebuild/test fan-out for unrelated targets.
/// `option_env!` resolves the same in any crate, so the
/// location is behavior-neutral. Cross-crate callers use [`folder_trust_inert`].
fn is_local_build() -> bool {
    // Runtime escape hatch: a pinned GROW_TEST_VERSION simulates a release build,
    // so tests/CI (which run unstamped, i.e. local-looking) can exercise the gate.
    if std::env::var(version::TEST_VERSION_ENV).is_ok() {
        return false;
    }
    option_env!("GROW_VERSION").is_none()
}

/// Resolve whether the folder-trust gate is enabled.
///
/// On a local/dev build (no `GROW_VERSION` release stamp) the feature is OFF
/// regardless of env/config/remote — a self-built grow auto-trusts (never
/// prompts, never gates repo-local MCP/LSP). Folder-trust applies only to
/// shipped, release-stamped binaries.
///
/// On a release-stamped build, normal precedence (via `BoolFlag`): env
/// `GROW_FOLDER_TRUST` > `[folder_trust] enabled` (user) > remote
/// `folder_trust_enabled` > default **true**.
pub fn feature_enabled(remote: Option<&RemoteSettings>) -> bool {
    feature_enabled_for_build(remote, is_local_build())
}

/// `feature_enabled` with the local-build flag fed in so both arms are unit-testable.
fn feature_enabled_for_build(remote: Option<&RemoteSettings>, is_local_build: bool) -> bool {
    // Local/dev builds never gate (auto-trust): folder-trust applies only to
    // shipped, release-stamped binaries. Even an explicit GROW_FOLDER_TRUST/config
    // opt-in is ignored here so a self-built grow never prompts.
    if is_local_build {
        return false;
    }
    fn from_toml(v: Option<&TomlValue>) -> Option<bool> {
        v?.get("folder_trust")?.get("enabled")?.as_bool()
    }
    let user = config::load_from_disk().ok();
    BoolFlag::env("GROW_FOLDER_TRUST")
        .config(from_toml(user.as_ref()))
        .feature_flag(remote.and_then(|r| r.folder_trust_enabled))
        .default(true)
        .resolve()
        .value
}

/// Persist an explicit `--trust` grant for `cwd`'s workspace so repo-local
/// servers are honored on the next resolve. Done client-side because trust is
/// durable: even when the agent runs in a separate leader process it reads the
/// same `~/.grow/trusted_folders.toml`. Best-effort; a write failure is logged,
/// not fatal.
pub fn grant_folder_trust(cwd: &Path) {
    // Local/dev builds never gate, so there is nothing to grant: `--trust` is a
    // no-op and the store is left untouched (the whole feature is inert).
    if folder_trust_inert() {
        return;
    }
    let key = workspace_key(cwd);
    let Ok(expected_identity) = workspace_identity_for_cwd(cwd, &key) else {
        return;
    };
    persist_trust(&mut TrustStore::load(), cwd, &key, &expected_identity);
}

/// Store-only half of revoking trust for `cwd`'s workspace: persist an explicit
/// `set_untrusted` ONLY when the folder was actually trusted, and report whether
/// it had been trusted. The in-process `DECISIONS` cache downgrade is the shell
/// wrapper's job (the cache lives there).
///
/// A revoke only records an explicit deny when it actually revokes a matching
/// grant; undecided/untrusted workspaces remain unchanged. Symmetric with
/// [`grant_folder_trust`].
pub fn revoke_folder_trust_store(cwd: &Path) -> bool {
    // Local/dev builds never wrote the store, so there is nothing to revoke.
    if folder_trust_inert() {
        return false;
    }
    let key = workspace_key(cwd);
    let Ok(expected_identity) = workspace_identity_for_cwd(cwd, &key) else {
        return false;
    };
    let mut store = TrustStore::load();
    let was_trusted = store.is_trusted_identity(&key, &expected_identity);
    // Persist a deny only when this call actually revokes the current entity's
    // grant; do not manufacture records for undecided/untrusted workspaces.
    if was_trusted && let Err(e) = store.set_untrusted_identity(&key, expected_identity) {
        tracing::warn!(
            path = %key.display(),
            error = %e,
            "folder trust: failed to persist untrust decision"
        );
    }
    was_trusted
}

pub fn persist_trust(
    store: &mut TrustStore,
    cwd: &Path,
    key: &Path,
    expected_identity: &WorkspaceIdentity,
) -> bool {
    if !workspace_identity_for_cwd(cwd, key).is_ok_and(|current| current == *expected_identity) {
        tracing::warn!(
            path = %key.display(),
            "folder trust: workspace identity changed before persistence; refusing grant"
        );
        return false;
    }
    match store.set_trusted_identity(key, expected_identity.clone()) {
        Ok(()) => {
            // Revalidate after the write so a source/repository replacement in
            // the validation-to-persist window cannot make this call report a
            // usable grant for a different entity. The record contains the
            // frozen identity, so a replacement during the write also remains
            // untrusted on every later store read.
            workspace_identity_for_cwd(cwd, key).is_ok_and(|current| current == *expected_identity)
                && store.is_trusted_identity(key, expected_identity)
        }
        Err(error) => {
            tracing::warn!(
                path = %key.display(),
                error = %error,
                "folder trust: failed to persist trust decision"
            );
            false
        }
    }
}

/// Whether any repo-local trust-sensitive config is present for `cwd`. When none
/// are present there is nothing to gate, so we skip the prompt entirely.
///
/// Detection short-circuits on the first marker because callers only need the
/// gate decision, not a presentation-oriented inventory.
pub fn repo_configs_present(cwd: &Path) -> bool {
    first_repo_config_kind(cwd).is_some()
}

/// Whether a project `.grow/config.toml` `[permission]` value would contribute
/// rules to the permission resolver. Mirrors the compact/verbose shapes that
/// `permission::resolution` loads: non-empty `allow`/`deny`/`ask` string arrays,
/// or a non-empty verbose `rules` array. Empty arrays / empty tables do not gate
/// (same as empty `[mcp_servers]` / empty `[plugins].paths`).
fn config_toml_permission_contributes(permission_value: &TomlValue) -> bool {
    let Some(table) = permission_value.as_table() else {
        // Non-table `[permission]` fails config load elsewhere; treat as a
        // marker so a malicious non-table still trips the gate rather than
        // resolving trusted.
        return true;
    };
    for key in ["deny", "allow", "ask"] {
        if table
            .get(key)
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty())
        {
            return true;
        }
    }
    table
        .get("rules")
        .and_then(|v| v.as_array())
        .is_some_and(|a| !a.is_empty())
}

fn path_present_or_uncertain(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

fn directory_present_or_uncertain(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_dir(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => true,
    }
}

/// Return the first trust-sensitive repo configuration kind, if one exists.
fn first_repo_config_kind(cwd: &Path) -> Option<&'static str> {
    // Resolve the git root + cwd→root dir chain ONCE and reuse it across the
    // git2-based marker checks below. A shared chain keeps the trust boundary
    // deterministic and avoids rediscovering the same repository per surface.
    // Cheap→expensive, short-circuiting on first hit.
    let chain = agent::repo::RepoDirChain::resolve(cwd);
    macro_rules! hit {
        ($kind:expr) => {
            return Some($kind)
        };
    }

    // Project `.grow/config.toml` declaring repo-controlled code-exec or
    // permission policy: a non-empty `[mcp_servers]` table, a non-empty
    // `[plugins].paths` array, OR a contributing `[permission]` section.
    // `[plugins].paths` loads as auto-trusted ConfigPath plugins; `[permission]`
    // allow/deny/ask rules auto-approve or block tools — a clone whose ONLY
    // repo-local config is either must still be gated (else it resolves Trusted
    // and the loader runs ungated).
    for path in crate::project_config::find_project_configs_in(&chain.dirs) {
        let Ok(root) = config::load_config_file(&path) else {
            continue;
        };
        let has_mcp_servers = root
            .get("mcp_servers")
            .and_then(|v| v.as_table())
            .is_some_and(|t| !t.is_empty());
        let has_plugin_paths = root
            .get("plugins")
            .and_then(|v| v.get("paths"))
            .and_then(|v| v.as_array())
            .is_some_and(|a| !a.is_empty());
        let has_permission = root
            .get("permission")
            .is_some_and(config_toml_permission_contributes);
        if has_mcp_servers {
            hit!("mcp");
        }
        if has_plugin_paths {
            hit!("plugins");
        }
        if has_permission {
            hit!("permission");
        }
    }
    // Project `.grow/lsp.json`.
    if cwd.join(".grow").join("lsp.json").is_file() {
        hit!("lsp");
    }
    // Project `.envrc` — auto-sourced in a bash subshell when `direnv` isn't
    // installed (direct code-exec), so an `.envrc`-only clone must still be
    // gated. The loader reads `<cwd>/.envrc` directly (NOT a git-root walk), so
    // probe at cwd to match exactly what gets executed.
    if cwd.join(".envrc").is_file() {
        hit!("envrc");
    }
    // Other project HOOK sources are resolved from the git worktree root only
    // (the chain's `git_root`, the same root hook discovery resolves from via
    // `workspace_key`), NOT cwd, so root-level hooks are gated even when launched
    // from a subdir. A repo-local hook file/dir is repo-controlled code-exec that
    // must be gated — else a hooks-only clone (e.g. `.grow/hooks/evil.json`) would
    // resolve trusted and run ungated. Presence mirrors discovery's "something to
    // gate" check.
    let hook_root = chain.git_root.as_deref().unwrap_or(cwd);
    if path_present_or_uncertain(&hook_root.join(".grow").join("hooks")) {
        hit!("hooks");
    }
    // Project PLUGIN dirs: project-scoped plugins are unified under folder-trust
    // too, so a repo-local plugin dir is repo-controlled code-exec (hooks/MCP)
    // that must be gated — else a plugin clone (e.g. `.grow/plugins/evil/`, even
    // one in a subdir launched via `cd sub && grow`) would resolve trusted and
    // run ungated. Uses the shared SSOT walk (cwd→git root) so detection matches
    // exactly what `discover_plugins` scans for Project scope (errs secure).
    if !agent::plugins::project_plugin_dirs_in(&chain.dirs).is_empty() {
        hit!("plugins");
    }
    // Project `.grow/agents` definitions can carry inline hooks and shadow a
    // built-in subagent, so an agents-only clone must still be gated.
    if !agent::discovery::project_agent_dirs_in(&chain.dirs).is_empty() {
        hit!("agents");
    }
    if directory_present_or_uncertain(&hook_root.join(".grow").join("workflows")) {
        hit!("workflows");
    }
    None
}

fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// MVP trust prompt: a plain stderr warning + stdin y/N read.
///
/// Defaults to NO on empty input, EOF, or any non-yes answer. Deliberately
/// minimal (no ACP modal); a richer modal is a future follow-up.
pub fn prompt_for_trust(key: &Path) -> bool {
    use std::io::{BufRead, Write};

    let mut err = std::io::stderr();
    let _ = writeln!(err);
    let _ = writeln!(
        err,
        "This folder contains repo-local config (.grow/config.toml / .grow/lsp.json / hooks) \
         that can run commands on your machine."
    );
    let _ = writeln!(err, "  Folder: {}", key.display());
    let _ = write!(
        err,
        "Trust the authors of this folder and allow these servers to start? [y/N] "
    );
    let _ = err.flush();

    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => false,
        Ok(_) => matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> DecideInputs {
        DecideInputs {
            expected_identity: Some(
                crate::trust::workspace_identity(&std::env::temp_dir()).unwrap(),
            ),
            store_trusted: false,
            repo_configs_present: true,
            is_interactive: false,
            key_recordable: true,
        }
    }

    fn repo_tmp() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        tmp
    }

    #[test]
    fn trust_precedence_is_explicit() {
        assert_eq!(decide(false, &inputs()), TrustOutcome::Trusted);
        assert_eq!(
            decide(
                true,
                &DecideInputs {
                    expected_identity: None,
                    ..inputs()
                },
            ),
            TrustOutcome::Untrusted,
        );
        assert_eq!(
            decide(
                true,
                &DecideInputs {
                    store_trusted: true,
                    ..inputs()
                },
            ),
            TrustOutcome::Trusted,
        );
        assert_eq!(
            decide(
                true,
                &DecideInputs {
                    key_recordable: false,
                    ..inputs()
                },
            ),
            TrustOutcome::Trusted,
        );
        assert_eq!(
            decide(
                true,
                &DecideInputs {
                    repo_configs_present: false,
                    ..inputs()
                },
            ),
            TrustOutcome::Trusted,
        );
        assert_eq!(
            decide(
                true,
                &DecideInputs {
                    is_interactive: true,
                    ..inputs()
                },
            ),
            TrustOutcome::Prompt,
        );
        assert_eq!(decide(true, &inputs()), TrustOutcome::Untrusted);
    }

    #[test]
    fn captured_identity_rejects_confirmation_period_replacement() {
        let parent = tempfile::tempdir().unwrap();
        let repo = parent.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git2::Repository::init(&repo).unwrap();
        let key = workspace_key(&repo);
        let inputs = decide_inputs_with_interactive(&repo, &key, true);
        let expected_identity = inputs
            .expected_identity
            .expect("workspace must be identified before prompting");

        std::fs::rename(&repo, parent.path().join("old-repo")).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        git2::Repository::init(&repo).unwrap();

        let mut store = TrustStore::load_from(parent.path().join("trust.toml"));
        assert!(!persist_trust(&mut store, &repo, &key, &expected_identity));
        assert!(!store.is_trusted_for_cwd(&repo, &key));
    }

    #[test]
    fn empty_repository_has_no_trust_sensitive_config() {
        let tmp = repo_tmp();
        assert!(!repo_configs_present(tmp.path()));
    }

    #[test]
    fn canonical_config_surfaces_are_detected() {
        for body in [
            "[mcp_servers.local]\ncommand = \"server\"\n",
            "[plugins]\npaths = [\"./plugin\"]\n",
            "[permission]\nallow = [\"Bash(*)\"]\n",
        ] {
            let tmp = repo_tmp();
            let grow = tmp.path().join(".grow");
            std::fs::create_dir_all(&grow).unwrap();
            std::fs::write(grow.join("config.toml"), body).unwrap();
            assert!(repo_configs_present(tmp.path()), "{body}");
        }
    }

    #[test]
    fn empty_canonical_tables_do_not_trigger_trust() {
        for body in [
            "[mcp_servers]\n",
            "[plugins]\npaths = []\n",
            "[permission]\nallow = []\ndeny = []\nask = []\n",
        ] {
            let tmp = repo_tmp();
            let grow = tmp.path().join(".grow");
            std::fs::create_dir_all(&grow).unwrap();
            std::fs::write(grow.join("config.toml"), body).unwrap();
            assert!(!repo_configs_present(tmp.path()), "{body}");
        }
    }

    #[test]
    fn canonical_executable_surfaces_are_detected_from_subdirectories() {
        for relative in [
            ".grow/hooks",
            ".grow/workflows",
            ".grow/plugins/local",
            ".grow/agents",
        ] {
            let tmp = repo_tmp();
            std::fs::create_dir_all(tmp.path().join(relative)).unwrap();
            let child = tmp.path().join("src").join("nested");
            std::fs::create_dir_all(&child).unwrap();
            assert!(repo_configs_present(&child), "{relative}");
        }
    }

    #[test]
    fn cwd_scoped_envrc_and_lsp_are_detected() {
        let env_repo = repo_tmp();
        std::fs::write(env_repo.path().join(".envrc"), "export X=1\n").unwrap();
        assert!(repo_configs_present(env_repo.path()));

        let lsp_repo = repo_tmp();
        let grow = lsp_repo.path().join(".grow");
        std::fs::create_dir_all(&grow).unwrap();
        std::fs::write(grow.join("lsp.json"), "{}").unwrap();
        assert!(repo_configs_present(lsp_repo.path()));
    }

    #[test]
    fn foreign_configuration_files_are_ignored() {
        let tmp = repo_tmp();
        for relative in [
            ".mcp.json",
            ".cursor/mcp.json",
            ".cursor/hooks.json",
            ".claude/settings.json",
            ".claude/agents/example.md",
            ".other-agent/config.json",
        ] {
            let path = tmp.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "{}").unwrap();
        }
        assert!(!repo_configs_present(tmp.path()));
    }
}
