# Close vendored nono inventory watch

## Why

The backlog asks whether 39 static draft deltas about every `third_party/nono` module should become Grow's sandbox contract after cross-platform dynamic testing. Those drafts include manifest, codegen and vendored implementation inventory, not concrete Grow failures. The accepted `sandbox-boundary` spec deliberately states Grow's process/child and hook-write guarantees without claiming equivalent platform backends. A blanket promotion would turn a pinned dependency's internals into product requirements without a user-visible acceptance case.

## What changes

Close this inventory watch. Preserve the static audit and draft deltas as historical evidence. Future sandbox fixes require a specific platform, profile, denial or execution scenario and a separate change; the concrete inherited-child-FD gap remains separately tracked.

This is backlog governance only: no code or accepted contract changes, so `skip_specs: true` is intentional.
