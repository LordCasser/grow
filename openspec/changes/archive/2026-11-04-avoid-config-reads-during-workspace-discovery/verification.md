# Verification

Date: 2026-11-04. Toolchain: cargo 1.93.1 / rustc 1.93.1, as pinned in rust-toolchain.toml.

## Reproduction

- The affected process logged `session.create.start` at 07:07:31.723 UTC and `session.create.done` after 78080 ms. During that interval the pager had no session ID and could not drain its queued input.
- The reported checkout's `.git/config` is a 137-byte dataless file (0 blocks, flags 1073741920). An independent Python read raised `TimeoutError: [Errno 60] Operation timed out`. Directory enumeration (2000 entries), marker stat and other ordinary metadata probes completed immediately.
- `sample` and `lsof` of the old `grow inspect` showed a synchronous read of that config. Installed binary inspect took 17.332 seconds; the pre-change debug binary took 18.838 seconds.
- Added `repo::tests::resolve_does_not_require_readable_git_config`, ran it before replacing `RepoDirChain`'s old implementation: **failed**, with `None` instead of the checkout root. The same check passes with path-only discovery.

## Automated checks

All commands use `~/.cargo/bin` first in PATH.

| Command | Result |
| --- | --- |
| `cargo test --locked -p agent -p workspace -p fsnotify -p diagnostics --lib -- --test-threads=4` | agent 495 passed; workspace 876 passed; diagnostics 55 passed; fsnotify 119 passed, 15 pre-existing ignored tests. No failures. |
| Same crate selection, filtered to `trust::tests::linked_worktrees_have_distinct_trust_keys_and_do_not_inherit_main_trust -- --exact` | Rechecked the subsequently extended linked-worktree test: 1 passed. |
| Same crate selection, filtered to `trust::tests -- --test-threads=4` | Final trust implementation: 43 passed, including retargeting to a common directory whose name ends in U+3000. Common-directory links trim only ASCII whitespace, matching libgit2. |
| `cargo build --locked -p cli --bin grow` | Passed, default production dependency graph. |
| `cargo fmt --all -- --check` | Passed after formatting changed files. |
| `git diff --check` | Passed. |
| `openspec validate --all --strict --no-interactive` | 17 items passed before archival. |

The full affected library suite covers normal/nested roots, home exclusions, separate and linked worktrees, aliases, project configuration and trust. Added tests cover malformed config, Agent definitions discovered from a nested cwd, relative gitfiles, bare repositories inside another checkout, invalid gitfiles, and unreadable config represented by a directory. Extending the real linked-worktree fixture verifies that an unavailable config leaves its identity unchanged, whereas retargeting `commondir` to another valid repository invalidates trust even though the `.git` marker entity remains unchanged.

## CLI checks

- Built debug binary: `/Users/lordcasser/workspace/projects/grow/target/debug/grow`.
- Read-only `grow --cwd <reported checkout> inspect --json`: exit 0 in **4.723 seconds**, correct `projectRoot`, 13 Agents and 20 skills. This exercised the user's existing configuration without displaying its contents.
- Linked a temporary probe to the built `pager` library and invoked the exact `Pick Agent` catalog entry point, `pager::views::agents_modal::build_switch_agent_catalog`, against the reported checkout and existing configuration: **9 selectable entries in 0.015 seconds**. The first probe compilation omitted the build's `panic=abort` setting and failed; correcting that harness setting compiled and ran successfully. No product change was needed.
- Isolated temporary checkout with malformed `.git/config`, temporary `GROW_HOME`, test provider at loopback port 1 and no prompt: real stdio ACP `initialize` / `authenticate` / `session/new` succeeded; new-session response took **0.267 seconds** and reported the correct root. After closing that process, a fresh process cold-loaded the same session in **0.177 seconds**.
- Same isolated configuration/storage with the actual reported checkout as cwd: real `session/new` succeeded in **1.961 seconds**. Closed the probe process without sending a prompt. Did not resume or write the user's existing sessions, send inference requests, change repository files, or run cold-load worktree cleanup against the user's checkout.

These are diagnostic observations, not a controlled release benchmark: old/new profiles and isolated/full configuration differ, and the real checkout still has unavailable cloud-backed data. Actual Git content operations retain their existing reads and failures. No claim is made that Git status/remote/commit operations now work on unavailable files, or that the existing 15 ignored watcher tests were exercised. The screenshot interaction itself was not driven through a terminal UI; the real catalog entry point, public discovery tests and real ACP/inspect entry points cover the changed behavior. The default CLI build and format check were rerun successfully after the final ASCII-whitespace adjustment.

## Adversarial review

- Path-only discovery validates Git topology with libgit2; it does not accept every directory named `.git` or parse gitfiles independently.
- The parent ceiling prevents a hit above the current level from being misattributed to a nested cwd. Encountering a bare repository stops the search instead of falling through to an enclosing checkout.
- Canonical cwd traversal resolves aliases; `RepoDirChain` retains original path spellings and its home exclusion.
- Folder trust still binds root, marker and common Git directory filesystem entities. Required topology errors remain errors, and a valid changed common directory fails the prior grant comparison.
- The patch adds no cache, timeout, retry, fallback repository settings or changes to persisted session/trust schemas. Actual Git content operations are left intact.

## Archival

`openspec archive avoid-config-reads-during-workspace-discovery --yes` completed with all tasks checked and merged both added requirements. Afterwards:

- `openspec validate --all --strict --no-interactive`: 16 passed, 0 failed.
- `openspec validate --archived --no-interactive`: 845 passed, 0 failed.

Removed this task's incremental compilation directories after building; retained the built CLI and normal dependency artifacts. No Git commit or installation over the user's existing `grow` binary was performed.
