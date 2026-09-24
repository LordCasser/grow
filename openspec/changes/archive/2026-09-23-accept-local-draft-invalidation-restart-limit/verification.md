# Verification

- `openspec/specs/client-surfaces/spec.md` scopes retry guarantees to a running
  Pager.
- `crates/codegen/pager/src/local_drafts.rs` stores retry deadlines in
  `LocalDraftRuntime.invalidations`; the existing binding/close/reopen test
  exercises retry recovery in the same process.
- The archived `retry-local-draft-invalidations/verification.md` explicitly
  records that sustained I/O failure followed by process death cannot guarantee
  deletion.
- Recovery restores composer state only; the archived local-draft design and
  current `restore_agent` implementation confirm it does not enqueue or send a
  prompt.
- `docs/development.md` already states that process exit during sustained I/O
  failure does not guarantee cleanup.
- No runtime behavior changed; no Cargo tests were run.
