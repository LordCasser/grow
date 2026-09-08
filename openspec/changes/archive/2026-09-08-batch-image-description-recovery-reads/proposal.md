## Why
Each uncached image group reloads and revalidates the same complete durable history. Multi-image recovery multiplies storage work by the group count.

## What Changes
Batch durable description lookup using one validated snapshot per recovery pass, preserving matching and integrity semantics.
