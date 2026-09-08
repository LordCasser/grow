## Why
Hide, show and announcement pruning each spawn independent whole-state persistence effects. Atomic files prevent partial documents but an earlier operation can finish last and overwrite a later preference.

## What Changes
Allow one announcement persistence effect in flight per AppView. While it runs, record that the current in-memory set changed; after completion, submit the latest set once. Route all three producers through this entrypoint. Continue a pending latest write after either success or failure.
