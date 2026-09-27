## Why

Permission decisions now run independently of the manager mailbox. A control API returns after enqueueing its command, before the actor cancels old request tokens. A completing old judgment can therefore authorize after reset, child release, or mode change returned. A deterministic reentrant classifier reproduced `Allow` for all three controls.

## What Changes

- Make permission control completion an actor acknowledgment: callers await the state transition and old-request cancellation before treating the control as complete.
- Preserve request-local model and prompt waits; the mailbox handles only short local control transitions.
- Add a fixed-order regression test for every affected control and retain the existing stale-response tests.

## Capabilities

### Modified Capabilities

- `tool-authorization`: define the completion boundary of permission control operations.

## Impact

`PermissionHandle` control methods and their Shell callers become async. No persisted permission format changes.
