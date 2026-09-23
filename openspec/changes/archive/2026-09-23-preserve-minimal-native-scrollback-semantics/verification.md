# Verification

## Scope and baseline

- `EntryRenderer` paints cached `BlockOutput.lines` after top vpad and optional group header; `joiner == Some("")` is the only no-separator continuation. The native commit path uses the same renderer and width as its live-tail height calculation.
- `commit_leading_run` advances the print-once frontier only when `insert_committed` reports success. The new semantic writer returns I/O errors to that existing boundary.
- `ratatui-inline` has distinct scrolling-region and non-scrolling-region dense insertion paths. Semantic insertion now uses the same row-count viewport accounting with feature-specific scroll operations; neither build configuration falls through to an empty insertion.
- Existing OSC 8 `LinkOverlay` is projected before serialization; capped footer rows exclude the clipped content's link spans.

## Static checks

| Command | Result |
| --- | --- |
| `rustfmt --edition 2024 --config skip_children=true` on this change's edited Rust files; subsequent `--check` on the same set | exit 0; `skip_children` avoids formatting unrelated modules in the shared dirty tree. |
| `git diff --check` scoped to this change's Rust, docs, and OpenSpec paths | exit 0. |
| `cargo test --locked -p ratatui-inline --lib semantic_insert -- --nocapture` | initial attempt exited 101 before compilation while the shared workspace lock was stale. After the parent synchronized Cargo.lock, exit 0, 2 tests passed; rerun after DECAWM-entry assertion also exit 0, 2 passed. |
| `cargo test --locked -p ratatui-inline --features scrolling-regions --lib semantic_insert -- --nocapture` | exit 0, 2 tests passed: semantic row insertion and long wrap chain under alternate scroll path. |
| `cargo test --locked -p ptyctl --lib scrollback_copy_text -- --nocapture` | exit 0, 1 test passed: emulator WRAPLINE distinguishes soft join and exact-width hard break. |
| `cargo test --locked -p pager-minimal --lib semantic_rows -- --nocapture` | exit 0, 2 serializer tests passed; the later full minimal run also passed all three semantic serializer tests. |
| `cargo test --locked -p pager --lib native_provenance -- --nocapture` | exit 0, 2 tests passed: source-space column end, plus vpad/group header/skip alignment. |
| `cargo test --locked -p pager --lib native_wrap -- --nocapture` | exit 0, 4 tests passed: exact-width hard break, long-token soft joiner, word-wrap hard separator, repeated decorative prefixes. |
| `cargo test --locked -p pager-minimal --lib -- --nocapture` | exit 0, 90 tests passed: full commit/frontier/error suite, footer, OSC 8 serializer, `/transcript` regressions. |
| `PAGER_BINARY=target/debug/grow cargo test --locked -p pager --test pty_e2e_minimal minimal_native_copy_wraps_long_token -- --ignored --nocapture --test-threads=1` | final exit 0, 1 test passed after fixing isolated harness setup. A 200-character source token copied intact from alacritty's WRAPLINE-bearing native history after 90 output rows; raw PTY stream contained a full `https://example.test/minimal-native-link-target` OSC 8 target. |
| `PAGER_BINARY=target/debug/grow cargo test --locked -p pager --test pty_e2e_minimal minimal_resize_preserves_committed_scrollback -- --ignored --nocapture --test-threads=1` | exit 0, 1 test passed after the same isolated setup: shrank from 50×120 to 30×80, committed content survived native reflow, process/prompt stayed usable, second turn streamed. |
| `openspec validate preserve-minimal-native-scrollback-semantics --strict --no-interactive` | exit 0, change valid |

## PTY setup diagnosis

The first copy-test run exited 101 after 40 seconds with empty native scrollback. Added read-only diagnostics showed the process was alive but `content.requests=[]` and `has_chat_completion=false`: the prompt had not reached inference. A second run exited 101 while waiting for the session context meter; the screen exposed the startup `Run Grow in a project directory?` picker. The generic `minimal · /help` sentinel can appear before this picker and before async session creation, so the injected prompt had been consumed by startup. Following the existing isolated `minimal_fps_hud_toggle` setup, both PTY scenarios now seed the mock LLM config and initialize a Git project in `content.home()`, then wait for the 128K context meter before injecting input. Both focused reruns passed. These were test-fixture preconditions, not a production native-writer failure.

The new PTY scenario reads alacritty's actual WRAPLINE flags through a read-only harness helper instead of joining physical text rows. It requests a WezTerm-capable OSC 8 route and checks a complete link target after a 90-row answer. The shared `target/debug/grow` was built once with `--locked` by the Kitty workstream and passed explicitly as `PAGER_BINARY`, avoiding the harness's implicit no-lock CLI build. This is emulator and PTY evidence, not proof of every physical terminal's trailing-space selection policy.

## Archive integration

- `openspec validate --all --strict --no-interactive`: exit 0, 18/18 before archive.
- `openspec archive preserve-minimal-native-scrollback-semantics --yes`: exit 0; added one `client-surfaces` requirement under `2026-09-23-preserve-minimal-native-scrollback-semantics`.
- Archived task 3.4 was checked only after the archive completed; full current validation passed 17/17 and archived validation passed 368/368 (both exit 0).
