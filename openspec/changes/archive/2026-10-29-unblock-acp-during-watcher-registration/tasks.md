## Tasks

- [x] Move leader config and skills watch registration onto the blocking pool; let the LocalSet return on shutdown while the blocking task retains eventual result ownership.
- [x] Make stdio skills watch registration race agent I/O completion and retain local watcher task shutdown ownership when registration wins.
- [x] Move in-process Pager config and skills registration onto the blocking pool, start the gateway first, queue config path updates, start config consumers and queue catch-up as soon as config registration is ready, and flush sessions on startup exits.
- [x] Add deterministic blocked-registration coverage for ACP progress, early agent completion, prompt LocalSet cancellation, and eventual watcher drop after registration returns.
- [x] Add deterministic in-process worker race coverage for gateway completion while watcher registration is blocked.
- [x] Add deterministic in-process startup-gap coverage for current global/project MCP catalog and auxiliary policy reconciliation.
- [x] Add deterministic in-process coverage that config policy hot reload remains live while skills registration is blocked.
- [x] Validate the change and have root run the focused startup tests.
