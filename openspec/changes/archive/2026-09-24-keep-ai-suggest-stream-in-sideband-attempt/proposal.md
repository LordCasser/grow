# Change: Keep AI-suggest stream consumption inside its Sideband attempt

## Why

The AI shell-command suggestion path opens its provider stream inside `SidebandRun::run_provider`, then decodes and collects that stream after `run_provider` returns. Raw bytes happen to remain captured by a cloned evidence handle in the HTTP stream, but task-local native terminal and any SSE end observation no longer have the attempt scope. The attempt is also marked returned before the provider body finishes. A failed or truncated stream therefore leaves incomplete causal metadata in the owning Timeline.

## What Changes

- Keep stream opening, SSE decoding, and response collection inside the same admitted Sideband provider future for Chat Completions, Responses, and Messages.
- Preserve the current short idle timeout, one-attempt behavior, and failure handling.
- Verify raw response bytes, native terminal, and observed stream-end evidence for complete and failed streams.

## Capabilities

### Modified Capabilities

- `model-sampling`: complete Sideband stream evidence and attempt lifetime.

## Impact

Only the existing AI-suggest provider path changes; no new storage format or retry policy is introduced.
