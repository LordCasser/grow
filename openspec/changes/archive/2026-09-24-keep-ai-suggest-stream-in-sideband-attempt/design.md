# Design

`run_provider` already admits the Sideband attempt, scopes `AttemptEvidence`, and marks the provider work returned after its future completes. Move the three backend stream transformations and `collect_response` calls into the future passed to this existing boundary. Do not add a second attempt owner or evidence sink. Normalize open and collection errors at the AI-suggest call site only after the scoped future returns; `SidebandRun::fail` still persists the attempt response evidence before the Sideband terminal.

The HTTP byte stream's cloned evidence handle already captures bytes independently of task-local scope. The fix matters for terminal observations made by L2 transforms, `[DONE]` and transport-error markers made during stream polling, and accurate attempt lifetime. The 5-second idle timeout remains at the L2 stream constructor. Any `run_provider` admission/cancellation error remains a Sideband failure instead of being mistaken for a provider response.

Test the real Sideband path against a streamed mock response for all three backends, including a malformed SSE case. Assert persisted evidence's raw bytes and metadata rather than relying only on returned text. A backend may complete at its native terminal without polling the underlying byte stream to EOF or `[DONE]`; `stream_end` remains null in that case rather than inventing a transport end.
