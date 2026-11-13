## Why

The newer OHOS CI image lets the first Homebrew install read the remaining dependency names from stdin. The Rust install then requests obsolete bottles for dependencies the loop skipped.

## What Changes

Isolate the dependency iteration input from Homebrew and keep the pinned source installation order. Issue the next patch after verifying the bootstrap. Product behavior is unchanged.
