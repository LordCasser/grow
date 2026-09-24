## Why

Local draft recovery validates bytes through an opened file handle but quarantines by pathname. If another process replaces that pathname between those operations, recovery can move a different, valid draft into quarantine and remove it from the active namespace.

## What Changes

- Bind quarantine to the filesystem entity opened and validated by recovery; if the active pathname now identifies a different entity, leave that entry untouched and do not quarantine it.
- Preserve bounded reads, regular-file symlink recovery, existing quarantine retention, and ordinary invalid-record handling.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `client-surfaces`: prevent quarantine from moving a replacement entry that was not the source validated by draft recovery.

## Impact

The local draft loader and quarantine tests in `crates/codegen/pager/src/local_drafts.rs`, the `client-surfaces` contract, and the developer explanation of local draft recovery. No storage format or dependency change.
