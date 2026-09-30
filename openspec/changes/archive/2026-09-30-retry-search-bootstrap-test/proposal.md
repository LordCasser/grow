# Proposal: retry transient search bootstrap in the recovery test

## Why

The recovery test invokes launch bootstrap directly and assumed its first call must complete. Under parallel test load, another temporary cache healing can advance the process-global cache epoch, making the documented `RunAgain` result valid. The test should follow the same bounded retry contract as the production bootstrap job.

## What Changes

The initial bootstrap in the focused recovery test retries `RunAgain` up to a fixed bound, then requires `Done` and the completed marker. Runtime behavior and archived behavior contracts do not change.

## Scope

Change only the initial bootstrap portion of `timeline_failure_preserves_row_and_recheck_indexes_repaired_content` to retry `RunAgain` a bounded number of times, then retain its `Done`, marker, row-preservation, and repaired-content assertions. No runtime behavior or archived behavior contract changes.

This is a test-only adjustment, so `.openspec.yaml` sets `skip_specs: true`; no contract delta is applicable.
