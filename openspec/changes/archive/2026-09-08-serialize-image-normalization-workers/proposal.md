## Why
Attachment normalization limits individual pixel counts but its spawn_blocking adapter admits unlimited concurrent sessions. Multiple large images can decode simultaneously despite the source comment describing one image at a time.

## What Changes
One process-wide semaphore permit for normalize/transcode blocking work. Move the permit into the blocking closure so canceling an async waiter does not admit another decode while the first still runs.
