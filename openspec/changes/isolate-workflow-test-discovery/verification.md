# Verification

Implementation base: `8fbc5218`. This is test-only maintenance; production discovery and the 512 KiB manifest limit are unchanged.

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed, 22 items.
- `openspec validate --archived --no-interactive`: passed before archive.

Rust compilation and runtime verification are pending in the shared integration worktree to avoid duplicate large Shell builds. Required selections are the full `session::workflow::` suite, `session::actor::tests::workflow_launch_tests::`, and `session::actor::tests::runtime_restore_tests::`. The new `sandbox_excludes_ambient_catalog_but_keeps_project_discovery` test includes an oversized user skill, user Agent, user Workflow and workspace-user skill control, then verifies isolation while retaining local resource discovery.

Platform limit: the polluted-home control is Unix-only because `dirs` 6 on Windows uses the system Profile Known Folder rather than USERPROFILE. Agent's `_with_home` and Skills' `_with_roots` seams are private and no test-support feature exports them to Shell. No Windows native isolation closure is claimed.
