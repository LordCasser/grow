# Keep native clipboard metadata calls off the Pager loop

## Why

The macOS clipboard hint and paste admission call `changeCount` / `types` on the Pager event loop. They skip an occupied in-process lock, but a call that acquires it can still wait inside lazy AppKit initialization or Objective-C messaging without a deadline. A stuck native call can therefore freeze input and rendering.

## What Changes

- Route Pager's metadata-only probes through one process-local, bounded worker. The calling thread waits at most 10 ms for its result; timeout or a busy queue returns the existing unknown result so later checks can retry.
- Keep the native version-before/types/version-after snapshot and in-process pasteboard serialization. A native call that never returns holds at most the one metadata worker; Pager never waits for it indefinitely.
- Make paste-time native image reads skip an occupied pasteboard lock and use the existing deadline-bound AppleScript fallback, avoiding a stack of blocking-pool threads queued behind one stuck native reader.
