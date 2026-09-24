## Context

The `grow/commands/list` handler already has two non-discovery paths: `kind="chat"` pulls the product catalog, and `sessionId` asks the session actor for its current advertised commands. The remaining pre-session path synchronously resolves folder trust and project plugin config, discovers plugins for a supplied cwd, then calls an async-shaped skill listing that performs filesystem work inline and a synchronous workflow scan.

The skills extension already owns a shared single-permit blocking worker and five-second deadline used for skill and workflow discovery. That worker retains its permit after the request times out because blocking filesystem calls cannot be canceled.

## Goals / Non-Goals

**Goals:** Keep all pre-session local catalog discovery off the request runtime thread, share the existing capacity bound, include queue wait in the five-second deadline, and distinguish timeout/worker failure from a valid empty catalog.

**Non-Goals:** Change chat catalog behavior, re-discover commands for a live session, alter discovery precedence, cache catalogs, interrupt filesystem calls, or refresh/mutate plugin installations.

## Decisions

- Keep the chat and `sessionId` branches before worker admission. They do not need the pre-session scan and retain their current semantics.
- Put folder-trust resolution, effective plugin config reads, pure cwd plugin discovery, skill discovery, and workflow discovery into one worker closure. This avoids leaving any synchronous disk work on the async path and preserves the required trust-before-project-config ordering.
- Reuse the existing single permit and deadline from `extensions::skills`; do not add another worker pool or semaphore. The closure owns the permit through completion, including after timeout/cancellation.
- Preserve `resolve_and_record(..., allow_prompt = false)`. A timed-out request cannot leave a terminal prompt running; an already-running trust decision may still finish as part of the detached blocking worker.
- Keep `build_for_cwd`, which is pure registry construction; do not call `refresh_and_build_for_cwd`, which refreshes local plugin installations.
- Map timeout and worker panic/join failure to explicit RPC errors. Preserve errors returned by command listing and never convert failures to empty success.

## Risks / Trade-offs

A filesystem operation cannot be forcibly stopped. A stuck worker can retain the sole shared discovery permit until process exit, so later pre-session listings fail by their own deadline instead of creating more concurrent scans. Normal Tokio runtime shutdown may wait for blocking tasks according to its configured shutdown policy.

## Migration Plan

No protocol or persistent data migration is needed. Deploy the handler change directly; callers already receive RPC errors for failed extension requests.
