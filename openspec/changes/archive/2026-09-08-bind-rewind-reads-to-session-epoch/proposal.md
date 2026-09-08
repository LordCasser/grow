## Why
Read UUID plus session ID rejects a different session but accepts an old read after unbind/rebind to the same session ID. Session binding already has an epoch to distinguish these lifetimes.

## What Changes
Capture the existing session_binding_epoch with each rewind read and require it at completion. Preserve valid reads across no-op same-ID binding and view switches.
