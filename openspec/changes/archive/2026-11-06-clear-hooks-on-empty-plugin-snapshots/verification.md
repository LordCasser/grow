# Verification

## Final parent acceptance

`cargo +1.93.1 test --locked --offline -p shell --lib plugin_snapshot_hooks -- --nocapture --test-threads=1` passed the real actor adoption test, exit0 (`/tmp/grow-backlog-plugin-hooks-final.log`). The parent then ran the same compiled test binary with `session::actor::tests::client_hooks_tests::`, `plugin_snapshot_hooks`, and `session::mcp_catalog::`: **28 passed / 0 failed, exit0**, `/tmp/grow-backlog-plugin-adoption-integrated.log`.

The first compile exposed a non-reexported PluginId path, corrected in the test. The initial positive command-hook control failed because the generic actor fixture leaves `hooks.resolved_workspace_root` empty; the test now supplies its owned TempDir as the actual execution cwd. The diagnostic outcome proved both handlers failed before spawn, rather than a plugin admission skip. Production Hook execution policy, validators and trust gates were unchanged. The test now requires actual successful handler outcomes as well as marker counts; only NotFound means zero markers, other read errors fail. None removal retains config hooks, updates generation/listing and stops plugin execution; Some(empty) and re-adoption remain paired controls.

Parent reviewed the narrow production move: collection stays conditional; contribution replacement and generation publication are unconditional. MCP diff and skills refresh paths remain intact. No claim of live-provider or Windows-specific reproduction is made.

## Initial delivery status (historical)

- `crates/codegen/shell/src/session/actor/hooks_plugins.rs::apply_plugin_registry_snapshot` now collects plugin `HookSpec`s only inside the `Some` branch, then unconditionally removes the `plugin/` contribution and appends the collected specs; `mark_hook_registry_changed` runs for every snapshot including None/empty. The no-registry bootstrap (config/file discovery before appending plugin specs) is preserved and still skipped when the snapshot has no specs.
- `rustfmt --check` clean on the modified production file and the new test file. No `cargo`/`cargo fmt` was run (serialization constraint; static verification only, so runtime tasks stay unchecked).

## Tier-3 test (written, not yet executed)

- `crates/codegen/shell/src/session/actor/tests/plugin_snapshot_hooks_tests.rs` (registered in `tests/mod.rs`) drives the real actor entry `apply_plugin_registry_snapshot` on `create_test_actor` with:
  - an observable plugin (inline SessionStart command hook writing a marker) as positive control;
  - a user-config control hook retained across adoptions;
  - None adoption: generation bump, `plugin/` specs gone from listing, plugin handler no longer executes, config hook still executes;
  - `Some(PluginRegistry::empty())` paired control: identical removal, empty plugin list;
  - re-adoption restores the plugin handler (removal is snapshot-scoped, not permanent damage).
- Fixtures use `std::env::temp_dir()` scratch dirs and gateway notification acks; no real plugin installation, no network, no user config reads (registry pre-seeded to avoid the bootstrap path's real config-layer discovery).

## Pending

- Parent-granted Cargo window: run the new test plus existing hooks/plugin/MCP-skills regression tests; MCP diff, skills refresh and notification serialization unchanged in code paths.
