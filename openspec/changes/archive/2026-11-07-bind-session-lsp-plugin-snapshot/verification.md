# Verification — AC-03a session LSP source isolation

父已接受生产最小修复和真实bootstrap进程里程碑。最终review F1修订日志 `/tmp/grow-session-lsp-review-final.log` 为1 passed、ACTUAL_EXIT=0、20.51s；源码phase1a在创建B之前完成A自己的初始化并核A/user，phase1b保留A再建B并核新增B与A不重复。父亲读两阶段实际记录、phase2/3与sharedlatest回包，保全部原控制。父额外逐PID核查最终16个记录PID均不存在，报告 `/tmp/grow-session-lsp-final-pid-review.json`；不是仅以gone/Z辅助谓词宣称waitpid完成。旧红、夹具失败归因和coverage限制仍如下保留。

归档已完成：`openspec archive bind-session-lsp-plugin-snapshot --yes` actual0，新增主规范configuration-rules的会话LSP Requirement，保留本目录完整证据。归档前strict **21/0**，归档后all strict **20/0**、archived strict **909/0**，命令均actual0。归档命令如实保留当时最后一个归档任务未勾选的5/6提示；归档操作完成后更新任务并复查通过。日志 `/tmp/grow-session-lsp-{prearchive-strict,archive,postarchive-all,postarchive-archived}.log`。开发说明已切换主规范链接，backlog移除AC-03a待修行并保闭合索引；AC-03b/AC-06继续独立开放，未发布2.6.2。

## Behavior under test

A new session's LSP backend is built from the plugin snapshot constructed for
that same session (cwd + folder trust + `_meta.pluginDirs`), never from the
process shared latest registry. Preserved unchanged: the
`lsp_tools_enabled && tool_ctx.lsp.is_none()` gate, user/project/plugin merge
precedence, project-trust filtering, launch/workspace cwd, `ProcessScope`
enrollment and the `ensure_started_background` ordering.

## Production change

`crates/codegen/shell/src/agent/mvp_agent/agent_ops.rs`,
`spawn_and_register_session`, LSP construction branch (only hunk of this task in
that file):

```rust
let active: Vec<_> = session_plugin_registry
    .as_deref()
    .map(|registry| registry.active_plugins())
    .unwrap_or_default();
```

replaces the previous `self.plugin_registry_handle.snapshot()` read. The session
registry is the value produced a few lines earlier by
`refresh_and_build_for_cwd(cwd, disk_config, parse_session_plugin_dirs(meta),
folder_trust::project_scope_allowed(cwd))` and already used for
`resolve_agent_definition`. No registry, cache, reload, public hook or helper
visibility change; the inherited `Arc<dyn LspBackend>` handling is untouched.
`None` / empty `active_plugins` means "no plugin contribution" — there is no
fallback to the shared latest.

`agent_ops.rs` carries other uncommitted hunks from unrelated work that predate
this task and were not touched here.

## Fixture

`crates/codegen/shell/tests/test_session_lsp_plugin_snapshot.rs` — one `#[test]`,
one isolated `HOME`/`GROW_HOME`, three phases in one sandbox. Phases share a
sandbox because `tools::util::grow_home()` caches its result in a process
`OnceLock`; a second test could not re-point the sandbox. The fixture drives the
real public ACP entry: `MvpAgent::new` + `connect_agent_v1`/`connect_client_v1`
duplex, `initialize` → `authenticate` → `session/new` with per-session
`_meta.pluginDirs`, `grow/session/close`, and `grow/plugins/list`.

Each plugin LSP command is a Python stdio JSON-RPC stub that appends one line on
spawn and one on `initialize` (label, pid, `os.getcwd()`, `rootUri`,
`workspaceFolder`). Session attribution uses the handshake `rootUri` only — the
manager's workspace root is the session cwd. The child's own process cwd is the
test process cwd (`LspClient::start_stdio` sets no `current_dir`) and is recorded
for diagnosis only. `tool_ctx.lsp_server_names` is not asserted on.

Marker parsing is strict: every complete line must be valid JSON (`panic` naming
the line on failure); a trailing unterminated fragment is retried on the next
poll. A complete but hijacked/leaked row therefore cannot be silently dropped.

Completion barrier (no sleeps): `LspManager::ensure_initialized` starts a
session's servers sequentially in `BTreeMap` key order and awaits each handshake
before the next. Both non-control keys (`project-lsp`, `snapshot-lsp`) sort
before `user-lsp`; the test asserts that ordering and waits for the user
handshake per cwd, so a wrongly included plugin server would already have
initialized at that point. Phase 1 creates session A alone and waits for its
`user-lsp`, then creates session B on the same cwd while A stays open and waits
for the second `user-lsp`: the same cwd is attributed per session instead of by
an aggregated label set (`review.md` F1).

Scope of the fixture: session plugin dirs stay per-session (`_meta.pluginDirs`)
and are never published. The launch-dir plugin is enabled with
`[plugins].enabled` and reaches the shared registry only through the real
`new_session` → `resolve_mcp_servers` → `ensure_plugin_registry` →
`SharedPluginRegistryHandle::reload` path; process `--plugin-dir` is not used.

## Cases and observed result (final green)

`/tmp/grow-session-lsp-review-final.log` (post-F1 bytes): `test result: ok.
1 passed`, `ACTUAL_EXIT=0`, 20.51s.

Initialize labels observed per handshake `rootUri`:

| Phase | Root (session cwd) | Servers observed |
| --- | --- | --- |
| 1a A alone, file `pluginDirs` | `shared-cwd` | `session-a`, `user` — each exactly 1, no `session-b` (no spawn either) |
| 1b B joins same cwd, A stays open, inline `pluginDirs` | `shared-cwd` | `session-a`, `session-b`, `user`; `session-a` still 1, `session-b` added exactly 1 |
| 1 launch-dir workspace backend | `neutral` | `user` |
| 2 different cwds, same server key | `project-c` | `session-c`, `user` |
| 2 different cwds, same server key | `project-d` | `session-d`, `project-d` (project `.lsp.json`), `user` |
| 3 no `pluginDirs` | `project-e` | `project-e`, `user` — no `leak-lsp` |
| 3 disabled plugin dir | `project-f` | `user` — no `leak-lsp`, no `session-disabled` |
| 3 control (plugin dir explicitly included) | `project-g` | `leak-lsp`, `user` |
| 3 launch-dir workspace backend | `launcher` | `user` |

Phase 1a is what makes the same-cwd case attributable: after A's last-key `user`
handshake its backend is done, and the only labels under `root_shared` are
`session-a` and `user`. Phase 1b then keeps A open and requires B's arrival to
change nothing except adding `session-b` once — swapping the two plugins'
commands would fail 1a, 1b, or both. Phase 3 asserts no `session-a…d` label ever
starts under `project-e` or `project-f`.

Shared-latest evidence (public `grow/plugins/list` with an unknown session id,
which falls back to `plugin_registry_snapshot()`), taken after the real prime and
printed in the same log:

```
shared plugins: [('leak-lsp', 'project', True)]
```

with `root` equal to the launch dir plugin. The assertion requires that entry to
be `enabled: true`, `scope: "project"` and that exact root, so the negative cases
rest on what production actually published rather than on config reasoning. The
same assertion requires the session-only plugins (`snap-file`, `snap-inline`,
`snap-disabled`) to be absent from the process registry.

## Old-red evidence

Kept as recorded at the production change; not re-taken.

1. Original pair (same-cwd / different `pluginDirs`), pre-fix production:
   `/tmp/grow-session-lsp-oldred.log` — marker `[]`, session A `rootUri = None`,
   `test result: FAILED`, `ACTUAL_EXIT=101`, 30.58s (wait timeout). Valid
   behavioural red: the sessions' own plugin roots were ignored because the
   bootstrap read the shared latest.
2. Extended matrix with old production temporarily restored:
   `/tmp/grow-session-lsp-oldred-matrix.log` — `0 passed; 2 failed`,
   `ACTUAL_EXIT=101`. Only its first failure is behavioural (session A labels
   `{"user"}` instead of `{"session-a","user"}`). The second failure is **not**
   behavioural evidence: `grow_home()` caches the first `GROW_HOME` for the
   process, so the second `#[test]` sandbox's user `lsp.json` was never read and
   its control session showed `{"leak-lsp"}` instead of `{"leak-lsp","user"}`.
   Fixed by the single-sandbox test; that log is not counted as a second product
   red.

## Fixture-side failures (not product defects)

| Log | Failure | Attribution |
| --- | --- | --- |
| `/tmp/grow-session-lsp-green.log` | `error[E0505]: cannot move out of sandbox.grow_home because it is borrowed`, `ACTUAL_EXIT=101` | Compile error in the fixture's borrow split (`plugin` closure vs `&sandbox.grow_home`). Fixed by removing the user-config write. This run overwrote the original pair-green log; the parent retained that run's output (`1 passed`, 4.24s, exit 0). |
| `/tmp/grow-session-lsp-final-green.commiterr.log` | runtime `session close must reap the LSP processes this fixture started: [(290, "Ss")]`, `ACTUAL_EXIT=101` | Fixture defect in the reaping assertion: it collected every recorded pid, including the launch-cwd `workspace::handle` LSP (pid 290, `rootUri …/neutral`) that no session owns. Fixed by restricting the assertion to session-owned pids (`rootUri` = session cwd). The surviving process was the workspace backend's, not a session leak; no session server survived. |
| `/tmp/grow-session-lsp-final-green2.log` | green before `rustfmt`, `1 passed`, `ACTUAL_EXIT=0`, 20.58s | Semantic pass on unformatted source. |
| `/tmp/grow-session-lsp-final-green3.log` | green on formatted source, `1 passed`, `ACTUAL_EXIT=0`, 20.53s | Same cases, before the shared-registry assertion. |

Backend lifetime note: `workspace::handle` builds its own LSP backend from
user + project LSP for the **agent launch cwd** with no session `ProcessScope`.
This fix neither creates nor manages it. The fixture excludes its pids from
session reaping and kills them by recorded pid at teardown.

## Process reclamation

- Session servers are attributed by `rootUri`. After `grow/session/close`,
  `close_and_reap` requires every session-owned pid to report no live state under
  `ps -o stat=` — "gone or `Z`" is read as *not running*; it is **not** proof that
  the child was `waitpid`-reaped, so this document does not claim a wait/reap for
  each pid. The assertion passed in all three phases.
- The launch-dir `workspace::handle` LSP pids were still running after both agents
  dropped (review-final log: `launch-dir workspace LSP pid … still Ss; killing
  fixture-owned process`); they were killed by their recorded pids, and
  `wait_after_kill` then saw no live state. That backend has no session
  `ProcessScope` and is not created or managed by this fix, so those pids are
  fixture cleanup, not session reclamation.
- Recorded stub pids for the review-final run (`spawn` events) — every one
  verified absent with `ps -o stat= -p <pid>` after the test:

  ```
  44242 44243 44253 44254 44262 44287 44296 44297
  44305 44306 44337 44338 44346 44347 44355 44356
  ```

  `44242` and `44337` are the two launch-dir workspace pids; the rest are session
  or launch-dir session-cwd servers.
- `ps ax | grep lsp_stub.py` after the run: no surviving stub.
- No name-based kill, no real user home/config access: `HOME`, `GROW_HOME`,
  plugin dirs and the marker all live in one `TempDir` sandbox.

## Regression round

`cargo +1.93.1 test -p shell --lib --locked --offline lsp` →
`/tmp/grow-session-lsp-regress.log`, `test result: ok. 5 passed; 0 failed;
4760 filtered out`, `ACTUAL_EXIT=0`. Includes the existing inherited-LSP
direction tests `agent::subagent::tests::subagent_inherits_parent_lsp_via_context`
and `agent::subagent::tests::no_parent_lsp_means_child_gets_none`.

## Commands and environment

All Cargo runs used:

```bash
env -u CARGO_TARGET_DIR -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC \
  -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 \
  CARGO_BUILD_JOBS=2 cargo +1.93.1 test … --locked --offline
```

| Purpose | Log | Actual exit |
| --- | --- | --- |
| old-red, original pair | `/tmp/grow-session-lsp-oldred.log` | 101 (FAILED, expected) |
| old-red, extended matrix (see caveat) | `/tmp/grow-session-lsp-oldred-matrix.log` | 101 |
| fixture compile error | `/tmp/grow-session-lsp-green.log` | 101 |
| fixture reaping error | `/tmp/grow-session-lsp-final-green.commiterr.log` | 101 |
| green, pre-format | `/tmp/grow-session-lsp-final-green2.log` | 0 |
| green, formatted | `/tmp/grow-session-lsp-final-green3.log` | 0 |
| green, with shared-registry assertion | `/tmp/grow-session-lsp-final-green4.log` | 0 |
| **green, final (post-F1 per-session phase 1)** | `/tmp/grow-session-lsp-review-final.log` | **0** |
| regression | `/tmp/grow-session-lsp-regress.log` | 0 |

Formatting: the fixture is formatted with `rustfmt +1.93.1 --edition 2024`
(workspace edition), and `/tmp/grow-session-lsp-review-final.log` runs on exactly
those bytes (`rustfmt --check` exits 0 afterwards).
`rustfmt +1.93.1 --edition 2024 --check
crates/codegen/shell/src/agent/mvp_agent/agent_ops.rs` → exit 0, so the production
hunk is formatted under the workspace edition. The production code is unchanged
since `/tmp/grow-session-lsp-final-green4.log`; only the fixture's phase 1 changed
for F1.

## Coverage limits

- Inherited backend (`tool_ctx.lsp.is_some()`): unreachable from the primary ACP
  entry — `spawn_and_register_session` builds a fresh context with no LSP and
  this branch is its only assignment. Existing pass-through is covered by the
  directed subagent unit tests above; no ACP-level coverage of that branch is
  claimed, and no test hook was added for it.
- Only the executed platform (macOS) is verified. Process checks use POSIX
  `ps`/`kill`; Windows / `TerminateJobObject` was not run.
- The fixture asserts server identity through handshake `rootUri` plus label; it
  does not exercise LSP request dispatch after startup, plugin reload, the Skills
  API (AC-03b) or config watch (AC-06), which are separate changes.
