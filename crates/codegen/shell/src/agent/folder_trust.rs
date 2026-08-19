//! Folder-trust gate ("do you trust this folder?").
//!
//! Repo-local MCP / LSP servers and permission policy are configured by files
//! an attacker can ship inside a cloned repository: project `.grow/config.toml`
//! sections for permissions, MCP, and plugin paths, plus `.grow/lsp.json`,
//! hooks, agents, workflows, plugins, and `.envrc`.
//! Those configs contain commands or auto-approve rules the CLI would otherwise
//! honor automatically — a 1-click RCE / policy bypass. This module resolves a
//! VS-Code-style trust decision ONCE per workspace, BEFORE any repo-local
//! server is spawned, and exposes a cheap [`project_scope_allowed`] check that
//! the MCP/LSP/permission loaders consult.
//!
//! Resolution lives here (not in `acp_session`) so the session core stays free
//! of feature logic; the loaders only call [`project_scope_allowed`].
//!
//! The DECISION side — the workspace scan, the pure [`decide`] precedence, the
//! interactive prompt, and the durable [`workspace::trust::TrustStore`]
//! reads/writes — lives in `workspace` (client-side); this module keeps
//! the CONSUME/gating side (the `DECISIONS` cache, [`resolve_and_record`], and
//! the loader filters). The ordered trust precedence is documented canonically
//! on [`workspace::folder_trust::decide`]; the consume-side nuance is
//! that two allows are PROVISIONAL (NOT cached): the "no repo configs" allow — so
//! configs appearing after the first resolve (git pull / agent write) are
//! re-checked on the next resolve rather than riding a stale grant — and the
//! unrecordable-key allow (cwd is $HOME / fs-root), which can never be persisted
//! anyway (see [`resolve_and_record_inner`] / [`compute`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use agent_client_protocol as acp;
use parking_lot::Mutex;
use workspace::trust::{TrustStore, is_unsafe_trust_root, workspace_key};

// Decision-side (scan/decide/prompt/store) relocated to `workspace`
// (client crate). `grant_folder_trust` is the ONLY moved item referenced from
// OUTSIDE this module (shell call sites + the pager's
// `shell::agent::folder_trust::grant_folder_trust`), so only it is
// re-published; the rest are private imports used within this module. A glob
// re-export is deliberately avoided: it would silently re-publish the
// cache-SKIPPING `revoke_folder_trust_store` next to the real
// `revoke_folder_trust` wrapper, inviting a stale-untrust security bug.
pub use workspace::folder_trust::grant_folder_trust;
use workspace::folder_trust::{
    DecideInputs, TrustOutcome, decide, decide_inputs, feature_enabled, folder_trust_inert,
    persist_trust, prompt_for_trust,
};

use crate::session::mcp_catalog::mcp_server_name;
use crate::util::config::{MCP_SCOPE_PROJECT, RemoteSettings};

// NOTE: this folder-trust store (`~/.grow/trusted_folders.toml`) is SEPARATE
// from the pre-existing per-plugin trust store
// (`agent::plugins::TrustStore` at `~/.grow/trusted-plugins`, plus the
// hooks' own project-trust gating). Trusting a folder here does NOT imply plugin
// trust and vice versa; the two are independent and non-contradicting.
// Unifying them is a tracked follow-up (out of scope for this PR).

/// Per-workspace resolved decision: `true` = repo-local (project-scoped)
/// servers are allowed to spawn. Keyed by canonical workspace key.
static DECISIONS: LazyLock<Mutex<HashMap<PathBuf, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Revoke trust for `cwd`'s workspace: downgrade the in-process decision cache
/// so a mid-session untrust takes effect immediately, while the store half —
/// persisting an explicit `set_untrusted` ONLY when the folder was actually
/// trusted — is delegated to
/// [`workspace::folder_trust::revoke_folder_trust_store`].
///
/// Without the cache downgrade a cached grant would short-circuit
/// [`resolve_and_record`] (which only reconciles untrusted→trusted), so hooks
/// would keep loading until restart. Unrecordable roots ($HOME / fs-root) are
/// refused instead: [`decide`] always trusts them and the store refuses both
/// their grants and denies, so a cache deny would be the one verdict nothing
/// (grant, store, prompt) could ever lift. Returns whether the folder had been
/// trusted. Symmetric with [`grant_folder_trust`].
pub fn revoke_folder_trust(cwd: &Path) -> bool {
    // Local/dev builds are fully inert: nothing was trusted-via-gate to revoke,
    // and recording `false` here would make `project_scope_allowed` wrongly gate.
    if folder_trust_inert() {
        return false;
    }
    let key = workspace_key(cwd);
    // Mirror the store's over-broad-root refusal: an unrecordable key resolves
    // Trusted by rule and can never be store-granted, so a cache deny here would
    // be permanent for the process with no in-product recovery.
    if is_unsafe_trust_root(&key) {
        tracing::warn!(
            key = %key.display(),
            "refusing folder-trust revoke for an over-broad root (never gated)"
        );
        return false;
    }
    let was_trusted = workspace::folder_trust::revoke_folder_trust_store(cwd);
    // Always downgrade the in-process cache so a mid-session untrust takes effect
    // immediately for this process, even for a cached grant with no backing store
    // record (e.g. a kill-switch / feature-off resolve). A later legitimate grant
    // reconciles it: the `Some(false)` arm of `resolve_and_record_inner` re-checks
    // the store.
    record(&key, false);
    was_trusted
}

/// Whether repo-local (project-scoped) MCP/LSP servers may spawn for `cwd`.
///
/// Authoritative and fail-closed, mirroring [`resolve_and_record_inner`]'s arms:
/// a cached **grant** short-circuits (allow); a cached **untrusted** verdict is
/// RE-READ against the store so a `grant_folder_trust` issued AFTER the untrusted
/// resolve is honored (it records the upgrade and allows); an **unrecorded** key
/// re-resolves via [`resolve_and_record`] — deny ONLY the dangerous case (feature
/// on AND repo-local code-exec configs present AND untrusted); allow no-configs /
/// unrecordable-key ($HOME / fs-root) / store-trusted / feature-off / inert. So
/// this never over-denies the common no-configs case, whose Trusted verdict is
/// provisional and therefore never cached.
///
/// The cache is consulted BEFORE delegating so a recorded `Some(false)` is
/// reconciled even on an inert build, where [`resolve_and_record`] would short-
/// circuit to allow before reaching the cache. `remote = None`: durable
/// feature-off / kill-switch verdicts are already cached by the launch/session
/// resolve that ran with the real RemoteSettings.
///
/// `DECISIONS` uses `parking_lot::Mutex` (no poisoning), so this gate cannot
/// fail OPEN on a poisoned lock.
pub fn project_scope_allowed(cwd: &Path) -> bool {
    let key = workspace_key(cwd);
    // Copy out of the lock so the Some(false) reconcile can re-acquire it
    // (parking_lot mutexes are not re-entrant).
    let cached = DECISIONS.lock().get(&key).copied();
    match cached {
        Some(true) => true,
        // Re-read the store so a grant issued after the untrusted resolve is
        // honored without a restart (mirrors `resolve_and_record_inner`).
        Some(false) => {
            if TrustStore::load().is_trusted(&key) {
                record(&key, true);
                true
            } else {
                false
            }
        }
        // Unrecorded: re-resolve fail-closed (no-configs / trusted / feature-off /
        // inert allow; untrusted + configs deny).
        None => resolve_and_record(cwd, None, false),
    }
}

/// Whether an agent's inline `hooks:` block may be appended to the live hook
/// registry. A PROJECT/cwd-discovered agent's inline hooks are repo-controlled
/// code-exec (and a project agent can SHADOW a built-in subagent, e.g. `explore`),
/// so they require folder trust; user/bundled/built-in agents (not cwd-sourced)
/// always keep theirs. `trusted` is evaluated LAZILY so non-project agents skip
/// the (filesystem-walking) trust verdict entirely. SINGLE definition shared by
/// the primary-session and subagent append sites (and the test) so they cannot
/// drift. The primary site passes its already-computed `hooks_trusted` verdict;
/// the subagent site passes `project_scope_allowed(parent_cwd)`.
pub(crate) fn agent_inline_hooks_allowed(
    scope: agent::config::AgentScope,
    trusted: impl FnOnce() -> bool,
) -> bool {
    scope != agent::config::AgentScope::Project || trusted()
}

fn record(workspace_key: &Path, allowed: bool) {
    DECISIONS
        .lock()
        .insert(workspace_key.to_path_buf(), allowed);
}

/// Test-only: force the recorded decision for `cwd`'s workspace key.
///
/// Tests use UNIQUE temp-dir keys and never globally clear `DECISIONS`, so they
/// can run in parallel without clobbering each other's recorded decisions.
///
/// Consumed by the MCP project-scope gate tests here and in `mcp_catalog`.
#[cfg(test)]
pub(crate) fn record_for_test(cwd: &Path, allowed: bool) {
    record(&workspace_key(cwd), allowed);
}

/// Resolve the trust decision for `cwd` ONCE and record it for the loaders.
///
/// Returns whether project-scoped servers are allowed. A cached **grant**
/// short-circuits; a cached **untrusted** verdict is re-checked against the
/// store so a later `--trust` grant is honored without a restart (see
/// [`resolve_and_record_inner`]). Persists on an accepted interactive prompt; an
/// explicit `--trust` grant is persisted up front by [`grant_folder_trust`].
///
/// `allow_prompt` must be `true` ONLY where a blocking stdin y/N read is safe —
/// i.e. agent `initialize` for the launch directory, before the TUI takes over
/// the terminal. Every other call site (per-session cwd, leader-served sessions
/// whose cwd differs from the launch dir, `grow mcp doctor`) passes `false`, so
/// an unresolved interactive-but-untrusted workspace resolves **fail-closed**
/// (untrusted, no prompt) — only the launch dir is ever prompted for.
pub fn resolve_and_record(cwd: &Path, remote: Option<&RemoteSettings>, allow_prompt: bool) -> bool {
    // Local/dev builds are fully inert: project scope is always allowed, so skip
    // the `trusted_folders.toml` read entirely.
    if folder_trust_inert() {
        return true;
    }
    let key = workspace_key(cwd);
    resolve_and_record_inner(
        &key,
        || TrustStore::load().is_trusted(&key),
        || compute(cwd, &key, remote, allow_prompt),
    )
}

/// Resolve the launch dir's project-scope trust verdict with a SINGLE expensive
/// gather, recorded into `DECISIONS` exactly as [`resolve_and_record`] would, so
/// the one-time deferred init helpers can share it. This is the AUTHORITATIVE
/// description of the launch-dir dedup + TOCTOU contract (the `MvpAgent`
/// field/method docs point here).
///
/// The deferred init helpers (`ensure_plugin_registry` and
/// `ensure_local_workspace_ops`) each gather independently, and the provisional
/// "no repo configs" allow is non-durable (never absorbed by the `DECISIONS`
/// cache), so without memoization the launch dir is scanned multiple times
/// during init. This gathers [`decide_inputs`] (store read + `repo_configs_present`
/// scan) ONCE and derives the verdict through the same [`resolve_and_record_inner`]
/// cache contract, so durable verdicts are recorded into `DECISIONS` and the
/// provisional allows (no-configs, unrecordable key) are left uncached.
///
/// TOCTOU: this records ONLY what [`resolve_and_record`] records, so a later
/// per-session `resolve_and_record(session_cwd)` still re-scans the provisional
/// no-configs case (config added post-startup via git pull / agent write is
/// caught). The init-time dedup belongs to the one-shot caller (a `OnceCell` on
/// `MvpAgent`), NOT to any new shared-cache entry.
pub fn resolve_launch_dir_trust(cwd: &Path, remote: Option<&RemoteSettings>) -> bool {
    // Local/dev builds are fully inert: project scope is always allowed, skipping
    // the store read + repo scan entirely.
    if folder_trust_inert() {
        return true;
    }
    let key = workspace_key(cwd);
    let feature = feature_enabled(remote);
    let inputs = decide_inputs(cwd, &key);
    // Re-read the store for the cached-untrusted reconciliation EXACTLY as
    // resolve_and_record does (so a `--trust` granted after a parallel resolve
    // recorded untrusted is still honored), and reuse the gathered inputs only
    // for the recompute — keeping the DECISIONS cache contract identical to
    // resolve_and_record without repeating the expensive repo_configs scan.
    resolve_and_record_inner(
        &key,
        || TrustStore::load().is_trusted(&key),
        || compute_from_inputs(&inputs, feature, &key, false),
    )
}

/// Cache-reconciling core of [`resolve_and_record`], split out so the
/// invalidation path is testable without the process-global trust store.
///
/// - A cached **grant** (`Some(true)`) is durable and short-circuits — neither
///   `store_trusted` nor `recompute` runs.
/// - A cached **untrusted** verdict (`Some(false)`) is re-checked via
///   `store_trusted`: a `grow --trust` grant issued AFTER this workspace was
///   first resolved writes the store, so honor it on the next session without a
///   restart. Without this re-read a long-lived leader would mask the grant.
/// - An **unrecorded** key (`None`) does a full `recompute`, which reports
///   `(allowed, durable)`; the verdict is recorded ONLY when `durable`. The
///   provisional "no repo configs" allow is non-durable, so it stays unrecorded
///   and every resolve re-checks for code-exec config that appeared after the
///   folder was first opened (TOCTOU).
fn resolve_and_record_inner(
    key: &Path,
    store_trusted: impl FnOnce() -> bool,
    recompute: impl FnOnce() -> (bool, bool),
) -> bool {
    // Copy out of the lock so `record` below can re-acquire it (parking_lot
    // mutexes are not re-entrant).
    let cached = DECISIONS.lock().get(key).copied();
    match cached {
        Some(true) => true,
        Some(false) => {
            if store_trusted() {
                record(key, true);
                true
            } else {
                false
            }
        }
        None => {
            // Record only durable verdicts; a provisional no-configs allow is
            // left uncached so the next resolve re-checks `repo_configs_present`.
            let (allowed, durable) = recompute();
            if durable {
                record(key, allowed);
            }
            allowed
        }
    }
}

/// Returns `(allowed, durable)`. `durable` is whether the verdict may be cached.
/// Two allows are NON-durable: (1) the "no repo configs" allow, because
/// repo-local code-exec config can appear after this resolve (git pull / agent
/// write) and caching that provisional grant would let a later `/hooks reload` or
/// new session run the new code with no trust decision (TOCTOU); and (2) the
/// unrecordable-key allow (cwd is $HOME / fs-root), which the store can never
/// persist anyway. Store-trusted, feature-off, and an accepted prompt are
/// durable; an untrusted verdict is recorded so a later `--trust` grant can
/// reconcile it (see [`resolve_and_record_inner`]).
fn compute(
    cwd: &Path,
    key: &Path,
    remote: Option<&RemoteSettings>,
    allow_prompt: bool,
) -> (bool, bool) {
    let feature = feature_enabled(remote);
    let inputs = decide_inputs(cwd, key);
    compute_from_inputs(&inputs, feature, key, allow_prompt)
}

/// [`compute`] split at the gather: derive `(allowed, durable)` from an
/// already-gathered [`DecideInputs`] so a caller needing more than one verdict
/// (see [`resolve_launch_dir_trust`]) pays for the expensive `decide_inputs`
/// gather (store read + `repo_configs_present` scan) only ONCE.
fn compute_from_inputs(
    inputs: &DecideInputs,
    feature: bool,
    key: &Path,
    allow_prompt: bool,
) -> (bool, bool) {
    match decide(feature, inputs) {
        TrustOutcome::Trusted => {
            // Within the Trusted arm the non-durable ("provisional") allows are the
            // "no repo configs" rule and the unrecordable-key rule (Case 2: cwd is
            // $HOME / fs-root, which can never be persisted) — both are feature-on
            // and not store-trusted; feature-off and store-trusted are durable.
            // Leave the non-durable allows uncached.
            let durable = !feature || inputs.store_trusted;
            (true, durable)
        }
        TrustOutcome::Prompt if allow_prompt => {
            if prompt_for_trust(key) {
                // Reload the store (the inputs gather dropped its copy) to
                // persist the accepted prompt grant.
                persist_trust(&mut TrustStore::load(), key);
                (true, true)
            } else {
                (false, true)
            }
        }
        // Untrusted, OR interactive where prompting is unsafe here (TUI owns
        // stdin) — the agent-`initialize` path owns the launch-dir prompt. Both
        // resolve fail-closed.
        TrustOutcome::Untrusted | TrustOutcome::Prompt => (false, true),
    }
}

/// PROJECT-scoped MCP server display names for `cwd` — the names dropped from a
/// merged server list when the workspace is untrusted.
///
/// SINGLE SOURCE OF TRUTH for "project-scoped MCP names" across ALL gate sites
/// (session merge, the session-less agent pool, `grow mcp doctor`). It MUST
/// enumerate every project MCP source the loaders read; adding a new repo-local
/// MCP source without extending this fn silently re-opens the gate (guarded by
/// `project_scoped_mcp_names_cover_every_source`).
///
/// Name-based (not `ConfigSource`-based) ON PURPOSE: it is the one primitive
/// that works for BOTH the sourced session merge AND the flat `load_mcp_servers`
/// agent-pool/doctor paths, which carry no `ConfigSource`. Names use the same
/// identity the merge dedups on ([`mcp_server_name`]).
///
/// Source: project `.grow/config.toml [mcp_servers]` (not the user-tier global
/// config).
///
/// Edge case: a name declared in BOTH a project config and the global
/// `~/.grow/config.toml` is dropped when untrusted. This is intended — untrusted
/// project content must not influence the command spawned for a shared name.
pub fn project_scoped_mcp_names(cwd: &Path) -> HashSet<String> {
    let mut names = HashSet::new();

    // `.grow/config.toml [mcp_servers]` entries tagged project (the loader's key
    // is the display name, matching `mcp_server_name` of the merged server).
    for (name, (_cfg, scope)) in crate::util::config::load_mcp_server_configs_with_project(cwd) {
        if scope == MCP_SCOPE_PROJECT {
            names.insert(name);
        }
    }

    names
}

/// Drop repo-local (project-scoped) MCP servers from a merged server list when
/// `cwd`'s workspace is untrusted. No-op when project scope is allowed. Mirrors
/// [`filter_untrusted_project_lsp`].
///
/// Matches on display name ([`mcp_server_name`]) — the same identity the merge
/// dedups and the disabled-name filters use — rather than the URL/key, so a
/// project server is dropped regardless of transport. Because the match is by
/// name, a server from ANY tier (client/plugin/user) whose name COLLIDES
/// with a project-declared name is ALSO dropped when untrusted: an untrusted repo
/// must not influence the command spawned for that name (see
/// [`project_scoped_mcp_names`]). Servers with no project-name collision are kept.
pub fn filter_untrusted_project_mcp(
    cwd: &Path,
    merged: Vec<acp::McpServer>,
) -> Vec<acp::McpServer> {
    if project_scope_allowed(cwd) {
        return merged;
    }
    let project = project_scoped_mcp_names(cwd);
    merged
        .into_iter()
        .filter(|server| {
            let name = mcp_server_name(server);
            if project.contains(name) {
                tracing::warn!(
                    server = %name,
                    "folder untrusted: skipping repo-local (project-scoped) MCP server"
                );
                false
            } else {
                true
            }
        })
        .collect()
}

/// Drop repo-local (project-scoped) LSP servers from a sourced LSP map when
/// `cwd`'s workspace is untrusted. Mirrors the MCP session gate; user- and
/// plugin-scoped servers are retained. No-op when project scope is allowed.
///
/// Thin `cwd`→verdict wrapper over the shared
/// [`tools::implementations::lsp::config::filter_project_lsp_when_untrusted`]
/// predicate, so Site B and the workspace build path share one gate.
pub fn filter_untrusted_project_lsp(
    cwd: &Path,
    sourced: std::collections::BTreeMap<
        String,
        (
            tools::implementations::lsp::config::LspServerConfig,
            tools::types::config_source::ConfigSource,
        ),
    >,
) -> std::collections::BTreeMap<String, tools::implementations::lsp::config::LspServerConfig> {
    tools::implementations::lsp::config::filter_project_lsp_when_untrusted(
        sourced,
        project_scope_allowed(cwd),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_tmp() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        git2::Repository::init(tmp.path()).unwrap();
        tmp
    }

    fn stdio(name: &str) -> acp::McpServer {
        acp::McpServer::Stdio(acp::McpServerStdio::new(
            name.to_owned(),
            std::path::PathBuf::from("/bin/true"),
        ))
    }

    #[test]
    fn inline_hooks_require_trust_only_for_project_agents() {
        assert!(!agent_inline_hooks_allowed(
            agent::config::AgentScope::Project,
            || false,
        ));
        assert!(agent_inline_hooks_allowed(
            agent::config::AgentScope::Project,
            || true,
        ));
        assert!(agent_inline_hooks_allowed(
            agent::config::AgentScope::User,
            || panic!("non-project scope must not resolve folder trust"),
        ));
    }

    #[test]
    fn cached_grant_short_circuits_and_cached_deny_reconciles() {
        let granted = tempfile::tempdir().unwrap();
        let granted_key = workspace_key(granted.path());
        record(&granted_key, true);
        assert!(resolve_and_record_inner(
            &granted_key,
            || panic!("grant must not read store"),
            || panic!("grant must not recompute"),
        ));

        let denied = tempfile::tempdir().unwrap();
        let denied_key = workspace_key(denied.path());
        record(&denied_key, false);
        assert!(resolve_and_record_inner(
            &denied_key,
            || true,
            || { panic!("cached deny must not recompute") }
        ));
    }

    #[test]
    fn provisional_result_is_not_cached() {
        let tmp = tempfile::tempdir().unwrap();
        let key = workspace_key(tmp.path());
        assert!(resolve_and_record_inner(&key, || false, || (true, false)));
        assert!(!DECISIONS.lock().contains_key(&key));
    }

    #[test]
    fn project_mcp_names_come_only_from_grow_config() {
        let tmp = repo_tmp();
        let grow = tmp.path().join(".grow");
        std::fs::create_dir_all(&grow).unwrap();
        std::fs::write(
            grow.join("config.toml"),
            "[mcp_servers.project]\ncommand = \"/bin/true\"\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join(".mcp.json"),
            r#"{"mcpServers":{"foreign":{"command":"/bin/true"}}}"#,
        )
        .unwrap();

        assert_eq!(
            project_scoped_mcp_names(tmp.path()),
            HashSet::from(["project".to_owned()]),
        );
    }

    #[test]
    fn untrusted_filter_drops_only_project_name_collisions() {
        let tmp = repo_tmp();
        let grow = tmp.path().join(".grow");
        std::fs::create_dir_all(&grow).unwrap();
        std::fs::write(
            grow.join("config.toml"),
            "[mcp_servers.project]\ncommand = \"/bin/true\"\n",
        )
        .unwrap();
        record_for_test(tmp.path(), false);

        let filtered =
            filter_untrusted_project_mcp(tmp.path(), vec![stdio("project"), stdio("user")]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(mcp_server_name(&filtered[0]), "user");
    }
}
