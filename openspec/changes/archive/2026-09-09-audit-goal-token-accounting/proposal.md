## Why
The user reported a billion-token provider dashboard versus a million-token Goal counter. The current Goal metric deliberately excludes cache reads, but the UI labels it only Usage.

## What Changes
Record a read-only reconciliation of the reported session and the exact accounting semantics. This audit changes no runtime behavior or budget contract; selecting a different budget basis requires a separate behavior change. No delta specs are introduced.
