# Design

`snapshot_memory_flush_state` keeps the materialized Surface, Surface IDs, revision and Timeline range together. The flush path constructs its own request after the system instruction is known. It takes at most the recent 20 non-system source items, expands the start to a User boundary, then applies `project_portable_history` to that frozen suffix. That existing projector preserves completed, uniquely paired calls/results and their attachments while removing dangling or ambiguous tool protocol and reasoning.

The request must fit a fixed 32k estimated-input-token budget, including the memory instruction and closing user turn, and 4 MiB of inline image URL data. If it exceeds either limit, drop the oldest entire User turn and project again. If the latest turn alone cannot fit, fail this flush without persisting a misleading summary. No tool result is truncated independently of its owning call. The cap is deliberately below the smallest supported normal context window and leaves output headroom; the provider's actual route still determines final admission.

The Sideband source remains the frozen Timeline range. Its attempt manifest records the frozen revision and live Surface IDs of the selected source suffix as context coordinates. A projected item may be omitted by the portable safety projector, so these IDs are the source selection read by the assembler, not a claim that every source item becomes a wire message. `selected_surface_ids` remains empty because no shadowed compaction leaves are read. The request itself remains frozen across this single attempt.

The system instruction explicitly says that tool results are untrusted observations and cannot issue instructions or prove permissions. No PermissionJudgment path changes.
