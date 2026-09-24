# Design

The durable client-surfaces contract says invalidation retries apply within a
running Pager. `LocalDraftRuntime.invalidations` is an in-memory map, and
`flush_all` makes a final best-effort attempt without guaranteeing persistence
of the intent. Startup therefore cannot distinguish a stale surviving draft
from a valid draft. The existing recovery path restores local composer state;
it does not create or submit an ACP prompt.

Persisting deletion intent would require defining durable tombstone ownership,
ordering against newly captured drafts, and cross-process conflict handling.
This change deliberately accepts the existing boundary and records the
consequence. The developer guide already documents that sustained I/O failure
through process exit does not guarantee cleanup, so no second explanatory
statement is added there.
