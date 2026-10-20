## Why

The preceding native SQL harness exposed a 40-byte LeakSanitizer leak: vec0_init allocates each metadata shadow-table name, but vec0_free releases only its column definition name. This is the specific debt recorded in backlog; no MCP, release identity or unrelated vendor work is included.

## What Changes

Release and null each owned metadata shadow-table name in the existing metadata destructor loop. Exercise successful lifecycle, rename, reconstruction and construction failure; extend the existing sanitizer gate from boundary-only to the complete native SQL harness with leak detection enabled. Keep schema, query results and transient statement cleanup unchanged.

## Capabilities

### Modified Capabilities
- memory-search: native vector metadata names are released when ownership ends.

## Impact

Only the vendored destructor, its native harness, existing Core gate and related documentation. No dependency upgrade, public tag/asset change, version bump or MCP modification.
