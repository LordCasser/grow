# Handle response replay projections in the session update test

## Why

`SessionUpdate` now includes the storage-only `ResponseReplayProjection` variant. The token-count extraction test matches the enum exhaustively, so adding this internal record makes the test target fail to compile.

## What Changes

Ignore response replay projection records while extracting `totalTokens` from ACP and Grow notifications. This is test maintenance and does not change runtime behavior or any user-visible contract.

## Contract impact

None. `skip_specs: true` avoids creating a behavior delta for a test-only exhaustiveness fix.
