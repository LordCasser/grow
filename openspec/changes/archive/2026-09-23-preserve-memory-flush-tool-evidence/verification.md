# Verification

- `RUST_MIN_STACK=33554432 cargo test -p shell --lib session::helpers::memory_flush::tests -- --test-threads=4`: 26 passed. The new cases inspect the serialized provider messages for a plan followed only by a successful tool result and inline image; they cover ambiguous/incomplete calls, whole-turn budget selection, recent User boundary and oversized attachment failure.
- `snapshot_memory_flush_state` keeps one `TimelineMaterialization`'s Surface, IDs, revision and range. `run_memory_flush` feeds the selected IDs and revision to `attempt_selected`; the Sideband's existing parent validator checks that context IDs are live in the frozen revision. No source IDs are synthesized from wire expansion.
- `git diff -- crates/codegen/chat-state/src/compaction_utils.rs` is empty: generic simplified compaction behavior is unchanged. The new memory-specific selector uses `project_portable_history`, whose pairing rules already cover completed unique calls and exclude dangling or ambiguous protocol.
- `openspec validate preserve-memory-flush-tool-evidence --strict --no-interactive`: passed before archival. `git diff --check`: passed.

The input cap is an estimator plus a direct inline image URL byte cap, not a provider tokenizer guarantee. A provider with a smaller actual window can still reject the request; that failure remains recorded as a failed MemoryFlush Sideband and does not write memory.
