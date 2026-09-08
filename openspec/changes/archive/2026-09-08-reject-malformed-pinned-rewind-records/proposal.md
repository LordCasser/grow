## Why
Pinned rewind history parsing currently skips JSON decode failures and reports success for the remaining records. This bypasses the newly propagated historical-load error boundary and can let rewind use a partial checkpoint projection.

## What Changes
Reject malformed nonblank records with InvalidData and the physical line number through the existing Result path. Keep the pinned source and live points on failure; retry after source repair. Blank lines and empty histories remain accepted.
