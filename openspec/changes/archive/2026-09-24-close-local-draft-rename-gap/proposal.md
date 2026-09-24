# Close the local-draft quarantine rename-gap watch

## Why
The backlog still tracks a pathname replacement between the last source-identity check and quarantine rename. The existing contract explicitly stops at the final check. This review decides whether extending it is proportionate to the local draft's trust and failure model.

## What changes
Record the decision and remove this narrow watch from the backlog. No runtime behavior or contract changes.
