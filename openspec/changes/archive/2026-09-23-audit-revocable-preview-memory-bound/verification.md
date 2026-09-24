# Verification: audit-revocable-preview-memory-bound

## Findings

- Regular session sampling supplies an evidence sink from `crates/codegen/shell/src/session/actor/turn/sampling.rs`. `crates/codegen/sampler/src/audit.rs` retains at most 64 MiB of raw response bytes per attempt and fails the stream when the cap overflows. `crates/codegen/sampling-types/src/lib.rs` separately caps the encoded outbound request body at 50 MiB; it is not an output cap.
- `crates/codegen/shell/src/session/actor/spawn.rs` creates unbounded SessionEvent and SamplingEvent channels. `ReplayBuffer` can coalesce streaming chunks to the configured thresholds (defaults: 2 KiB, 100 entries, 10 ms), but when settings are absent it forwards every incoming chunk immediately.
- `crates/codegen/shell/src/session/persistence.rs` retains candidate chunks in `pending_sampling: Vec<_>` and explicitly does not coalesce them. No aggregate byte or entry limit applies to this vector. At projection commit it clones each staged notification into another vector before storage commit.
- `crates/codegen/shell/src/session/response_projection.rs` copies each admitted assistant/reasoning string into replay updates. `crates/codegen/shell/src/session/storage/jsonl/mod.rs` clones the projection for storage serialization. Therefore the existing 64 MiB evidence limit does not bound all simultaneously retained representations or small-chunk metadata overhead.
- No focused runtime measurement was added: the audit established no ready allocation/RSS harness that isolates these live owners without building the full shell. The exact peak multiplier and worst-case staged notification count remain unmeasured and are recorded as follow-up work.

## Checks

- Source paths and constants inspected directly; no Cargo command was needed.
- `git diff --check -- openspec/backlog.md openspec/changes/archive/2026-09-23-audit-revocable-preview-memory-bound`: passed.
- `openspec validate audit-revocable-preview-memory-bound --strict --no-interactive`: passed.
- `openspec validate --all --strict --no-interactive`: passed, 18/18 before archive and 17/17 after archive.
- `openspec archive audit-revocable-preview-memory-bound --skip-specs --yes`: completed; no spec update was merged.
- `openspec validate --archived --no-interactive`: passed, 426/426.
