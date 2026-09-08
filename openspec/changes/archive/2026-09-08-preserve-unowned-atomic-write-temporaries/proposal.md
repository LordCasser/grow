## Why
config::fs_atomic::write_atomically removes its computed temporary path on any failure, including create_new failure. An existing colliding file can therefore be deleted even though this attempt never owned it.

## What Changes
Return immediately on temporary creation failure; cleanup is reached only after exclusive creation succeeds. Preserve caller-facing Result, temp naming, optional Unix mode and final rename behavior. Cover collision, publication failure and successful replacement with explicit temp-directory paths.
