## Why
The real ImagePreview input branch calls ImageViewerState::open synchronously. That method copies/reads encoded data and can invoke a10-second sips process before returning. Existing background viewer loading is wired to completion handling but no production constructor admits a loading viewer.
## What Changes
Connect the real prompt-image opening entry to the existing background pipeline for both in-memory and durable-file sources. Snapshot the source and requesting terminal protocol; return a loading viewer immediately. Preserve image number and enforce a unique owner per opening so late results cannot replace a reopened viewer.
## Scope
Viewer admission, load source/protocol transport and completion integration. Do not remove unapproved candidate R20. File-byte budgets are a separate follow-up, not solved by moving I/O off the input thread.
