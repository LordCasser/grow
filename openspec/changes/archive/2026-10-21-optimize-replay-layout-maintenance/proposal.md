# Reduce Pager scrollback layout maintenance cost

## Why

Long-session resume and ordinary rendering still pay for layout work that scales with the full visible history. A clean frame re-sums every visible entry, lazy measurement rebuilds all virtual offsets after each pass, and the height-only streaming path scans prompt descriptors repeatedly. These costs grow with history size even when the user sees only a small viewport.

## What Changes

Optimize the internal `ScrollbackState` layout-maintenance path while preserving the existing layout result and interaction semantics. Keep lazy exact measurement and the current resume warm-page policy. Use the already maintained virtual positions for range totals, patch only suffix positions after height changes, and update prompt descriptors in one ordered pass. Structural changes and width changes continue to use a full rebuild.

Before production changes, add and run an ignored in-process benchmark for unchanged clean frames and height-only updates at 128, 512, and 2048 entries. The benchmark checks exact virtual offsets and total height against an independent renderer-based oracle. Record before/after results in this change.

The owning task will also add the separately scoped recovery-loading input behavior and its `client-surfaces` delta to this change. The layout portion does not change that input contract.
