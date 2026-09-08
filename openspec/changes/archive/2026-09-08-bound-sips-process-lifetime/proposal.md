## Why
Sips conversion currently uses blocking Command::status with no deadline. A stuck converter retains a blocking worker and its private image files indefinitely.
## What Changes
Give the spawned converter a 10-second execution deadline. Own its detached process group, kill the group on exit/error/timeout, and reap a still-running leader before returning. Existing Rust conversion fallback remains after sips failure.
