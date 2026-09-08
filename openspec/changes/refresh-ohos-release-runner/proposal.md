## Why
Release run 34232309109 cannot start the OHOS build: the pinned Harmonybrew runner digest was garbage-collected by its registry (manifest unknown).

## What Changes
Refresh only the immutable runner digest from the same upstream repository. The current manifest resolves to Linux arm64. Product behavior and release signing/verification gates are unchanged; this is build tool maintenance.
