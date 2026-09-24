# Verification

## Evidence

- The current contract is [client-surfaces](../../../specs/client-surfaces/spec.md), requirement `Sampling previews are isolated by attempt and delivery capability`, scenario `Reconnect misses the candidate terminal boundary`: reconnect drops an unconfirmed old preview and displays admitted history.
- The archived [sampling recovery design](../2026-09-10-unify-sampling-attempt-recovery/design.md) explicitly says it does not add a protocol for active candidate-prefix replay. Pager implements the boundary in `crates/codegen/pager/src/acp/tracker.rs` (`has_unconfirmed_sampling_preview`, `discard_unconfirmed_sampling_preview`), `crates/codegen/pager/src/app/root/event_loop.rs` (`plan_reconnect_load`), and `crates/codegen/pager/src/app/agent_view/session.rs` (`begin_session_reload`, reload resolution). The archived verification lists reconnect replay and failed-load ownership regressions.
- The current local-draft requirement in [client-surfaces](../../../specs/client-surfaces/spec.md) is explicitly scoped to a running Pager. The archived [draft invalidation verification](../2026-09-08-retry-local-draft-invalidations/verification.md) states that in-memory intent cannot guarantee deletion after process death. `crates/codegen/pager/src/local_drafts.rs` stores retries in `LocalDraftRuntime.invalidations` and retries them in-process; existing `failed_rpc_invalidation_survives_binding_close_and_reopen` covers binding/agent reopening while the process stays alive, not restart recovery.
- The current [extension-runtime](../../../specs/extension-runtime/spec.md) requirement guarantees abandoned-call cancellation notifications for stdio/Streamable HTTP and includes `ACP reverse bridge limitation`. The archived [cancel change](../2026-09-23-cancel-abandoned-mcp-tool-requests/verification.md) records that ACP limitation as separate. `crates/codegen/mcp/src/acp_transport.rs` documents its half-duplex boundary and drops every id-less notification; `notifications/cancelled` has no JSON-RPC id. Existing bridge tests confirm notification dropping but do not claim that a real ACP SDK tool stops after cancellation.

## Results

- Change validation: `openspec validate reconcile-sampling-reconnect-backlog --strict --no-interactive` passed.
- Full strict validation before archive: `openspec validate --all --strict --no-interactive` passed, 19/19 items.
- Relative local Markdown link check found no missing targets; `git diff --check` passed.
- No Rust tests were run because this change only edits backlog documentation and OpenSpec records.
- Archived with `openspec archive reconcile-sampling-reconnect-backlog --skip-specs --yes`; no spec was merged because this is documentation-only.
- Final post-archive `openspec validate --all --strict --no-interactive` passed, 17/17 active items; `openspec validate --archived --no-interactive` passed, 385/385 archived items; all local Markdown links resolve and `git diff --check` passed.
