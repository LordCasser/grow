## Why

The pinned Harmonybrew core revision can reference OHOS bottles that are no longer available from its mirror. The OHOS release job then stops during toolchain bootstrap before compiling Grow, even though the pinned formulas retain usable source inputs.

## What Changes

- Build the pinned OHOS toolchain runtime formulas from source in the release bootstrap so missing bottles do not block the build.
- Keep the existing pinned formula revision and Rust version contract.

This is a release build infrastructure change. It does not change Grow runtime behavior or its interface, so no spec delta is needed.
