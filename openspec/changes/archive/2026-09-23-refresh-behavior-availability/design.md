## Context

`BehaviorAvailability` is Shell-authored metadata on `AvailableCommandsUpdate`. Shell remains authoritative and recomputes admission before applying a selection. Pager caches the latest projection for command and settings rendering.

Prompt promotion installs `ForegroundState::RegularTurn` in `notification_drain.rs`, but that admission path did not publish a command update. Goal continuation also installs a regular foreground directly. Normal turn and manual compaction completion arbitrate successor work before knowing whether the session stays busy or returns to idle. Step-control admission changes `pending_step_controls`, which participates in Behavior assessment. These are published through the existing command update after releasing the admission-state lock. Projection creation also has asynchronous boundaries: an older snapshot can finish after a later snapshot, and Pager previously replaced its cache unconditionally.

## Decisions

- Allocate a per-session projection revision at the start of each projection build, before asynchronous capability and workflow reads. Carry it in `BehaviorAvailability`.
- Publish the existing `AvailableCommandsUpdate` after queued prompt/Goal promotion installs regular foreground ownership and before the turn starts. After terminal or compaction arbitration, publish only if the session still owns no foreground. Publish after pending step-control admission and after the control drain releases its foreground fence. These use the existing transport and refresh already-open Pager surfaces.
- Have Pager replace its cached projection only for a strictly greater revision. Older serialized clients/fixtures without a revision deserialize as revision zero; the first received projection is still accepted.
- Do not change Shell admission rules or add a generic projection/event mechanism. Shell continues to revalidate each Behavior request against a current snapshot.
- Session termination and fatal teardown close the session admission surface; no further picker interaction is admitted after that lifecycle transition, so teardown does not publish a final availability update.

## Risks and Trade-offs

- The revision orders projection builds, not durable admission state. Its only purpose is to prevent completion-order races from regressing the Pager cache.
- `u64` exhaustion is not realistic for a session; checked revision advancement fails rather than wrapping to an older value.
- `AvailableCommandsUpdate` is also used for slash commands, so an empty available-command set must still be publishable with its metadata for the refresh guarantee to hold.

## Validation

- Shell test: drive prompt promotion from idle with Plan selected and verify the published projection reflects regular foreground admission.
- Pager test: open Settings, accept a newer projection, then deliver an older one and verify the open snapshot stays on the newer availability.
- Run focused Shell and Pager tests after shared Cargo target is free; run strict OpenSpec validation before and after archive.
