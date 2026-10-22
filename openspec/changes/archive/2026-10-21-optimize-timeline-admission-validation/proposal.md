## Why

Bulk Timeline replay still performs repeated historical work when it validates identity-bearing assistant responses. Each response searches prior events for the matching completed attempt and scans them again for duplicate admission identity; it also clones the current Surface and serializes malformed tool exchanges merely to compare a quarantine count. This makes replay cost grow with both event history and Surface size.

## What Changes

- Measure cold `Timeline::from_events` on repeatable valid response-admission histories at 128, 512, and 2048 responses before changing production code.
- Replace repeated historical lookups with admission state maintained by the Timeline fold.
- Count malformed tool exchanges without cloning or serializing the Surface. Only add a healthy-append identity summary if measurements show the remaining Surface scan is material.
- Keep current validation errors, folded Surface, branch behavior, compaction and rewind semantics, and live failed-write atomicity.

## Capabilities

None. This change only alters internal validation cost and adds an ignored performance probe; it does not change the persisted format or behavior contract, so `.openspec.yaml` sets `skip_specs: true`.

## Impact

The implementation is limited to `crates/codegen/chat-state/src/timeline.rs` and `crates/codegen/chat-state/src/compaction_utils.rs`. The Timeline and context-compaction specifications remain authoritative. No storage schema or external API changes are intended.
