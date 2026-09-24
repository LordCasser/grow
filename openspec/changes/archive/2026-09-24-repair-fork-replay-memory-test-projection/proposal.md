## Why

The fork replay memory integration test still treats compact `ProjectedText` as an ACP notification and fails to compile after the archived response-projection change.

## What Changes

Update only the test's reference conversion to construct the equivalent ACP text update from each compact projected chunk. No runtime behavior or contract changes.
