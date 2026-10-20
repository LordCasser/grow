# Bind release binary identity to the validated source

## Why

OHOS v2.4.1 prints its version correctly but falls back to unknown for Git identity. cli/build.rs silently discards git command failures; the official workflow already validates a full immutable RELEASE_COMMIT but does not carry it through container build boundaries. Existing smoke only checks the version or prints output, so unknown was accepted.

## What Changes

Consume a validated full RELEASE_COMMIT in the CLI build script, track it with Cargo's environment invalidation, pass it through every container builder and assert exact version plus short source identity in official smoke. Explicit invalid metadata fails compilation. Local builds without explicit metadata retain Git discovery and the existing Git-unavailable fallback. Published v2.4.1 remains immutable; this only fixes future builds.
