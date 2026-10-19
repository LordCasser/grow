# Recover live sessions after exhausted sampling

### Why

Completed provider responses with invalid tool-argument JSON stop after at most three total attempts. Existing tests cover the sampler cap and adjacent next-turn recovery separately; some next-turn tests manually replace foreground ownership with Idle and disable retries. They do not prove that the production actor settles the failed Turn and admits a later user prompt without restarting Grow.

The outer sampling-error path also exhausts a child's output grant unconditionally, even when attempt settlement already recorded exact usage. A recoverable provider-generation failure must not turn known spending into unknown spending.

### What Changes

- Specify and test manual continuation through the same resident session after exhausted malformed generation, including normal foreground settlement and new request budgets.
- Preserve accurately settled child spending and remaining output grants across provider failure; keep unknown usage, real budget exhaustion and failed settlement closed.
- Verify Goal-internal failure pauses automatic continuation, ordinary user input remains possible, and explicit Goal restart preserves its identity and spending constraints.
- Use production session admission, run loop, sampler, loopback providers and Timeline persistence. Tests must not reset Idle or replace the session actor between failed and successful prompts.

## Capabilities

### Modified Capabilities

- `model-sampling`: live-session manual recovery and child-grant accounting after failed sampling.
- `behavior-goal`: recovery from a provider failure preserves the distinction between manual interaction and Goal activation.

## Scope

This change owns the focused recovery tests, any minimal defect fixes they establish, developer guidance and verification evidence. MCP work, new retry frameworks and unrelated tool-execution lifecycle debt remain outside this change.
