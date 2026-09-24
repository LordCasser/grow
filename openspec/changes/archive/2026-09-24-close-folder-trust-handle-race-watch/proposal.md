# Close Folder Trust handle-relative race watch

## Why

The backlog proposes a held workspace directory capability across every trusted project loader and process spawn. The identity cache and trust-store CAS already stop a replacement present at decision time, but pathname-based config opens and child cwd selection retain a narrow concurrent rename window. The remaining question is whether this warrants a cross-platform capability redesign.

## What Changes

Record a no-go decision for that redesign in the current Folder Trust threat model, state the residual risk precisely, and remove the watch from the backlog. This is an assessment and developer-documentation change, not a runtime behavior or accepted contract change; `skip_specs: true` applies.
