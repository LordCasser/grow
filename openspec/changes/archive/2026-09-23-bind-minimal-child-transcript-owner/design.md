# Design

`TranscriptBuild` already freezes its root Agent ID and entry IDs. Add the optional child view key, selected view Agent ID and session ID. The request resolver follows the same precedence as input: an active root permission queue owns the command, otherwise an existing active child owns it, otherwise the root does.

Each pump slice resolves the captured view, verifying both the child key and current session ID. Entry IDs and cwd/media paths are read only from that view. A root reload invalidates an in-flight build through the existing hook; an active reload pauses the pump and discards its staged prefix. A missing or rebound owner drops the build instead of retargeting a recycled entry ID. Pager handoff records the selected view in `PendingPager`, so later failure feedback uses its existing owner check. No new persistent state or terminal rendering mechanism is required.

Validation covers distinct parent/child content and cwd, switching focus between slices, root reload, and child replacement/removal. The native Minimal commit/live owner problem is deliberately outside this change.
