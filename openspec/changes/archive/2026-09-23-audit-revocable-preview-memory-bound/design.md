# Design

Follow one ordinary session sampling attempt and keep each representation distinct:

1. `turn/sampling.rs` installs `sampling_evidence::sink`; `AttemptEvidence` retains raw response bytes up to `MAX_RESPONSE_EVIDENCE_BYTES` (64 MiB). The request body's separate 50 MiB ceiling is an outbound limit and does not cap generated output.
2. The sampler decodes streaming response events and emits text/thought chunks. `SessionEvent` and `SamplingEvent` use unbounded channels. The normal `ReplayBuffer` defaults to 2 KiB/100 entries/10 ms when buffering settings are present, but `None` disables coalescing and sends each chunk onward.
3. `SessionPersistence.pending_sampling` is a `Vec<PendingSamplingNotification>` with no byte or item cap. Candidate notifications are intentionally not coalesced. The attempt lifecycle discards or accepts this vector at a later boundary.
4. Projection construction copies admitted assistant/reasoning strings into a fresh `updates` vector. `commit_response_projection` clones staged notifications into a second vector; the JSONL storage adapter clones the projection before durable serialization.

The 64 MiB evidence ceiling therefore bounds the retained raw audit body for configured session attempts, but it does not establish an end-to-end transient-memory ceiling or per-candidate staged item count. Fine-grained stream fragmentation can increase per-notification and queue overhead. This audit does not infer an exact RSS multiplier because allocator capacity, parser buffers, scheduler interleaving, concurrent session ownership, and serialization buffers were not measured.

The backlog should retain this as unresolved resource work, phrased around the missing end-to-end admission bound and measured peak-copy behavior. A safe follow-up must preserve revocability until canonical admission and must not expose candidate bytes through replay or external durable storage before acceptance.
