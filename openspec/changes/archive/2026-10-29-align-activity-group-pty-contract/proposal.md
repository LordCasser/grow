## Why

The mixed-activity PTY fixture still asserts the grouping behavior that preceded `unify-tool-thought-aggregation`: it expects Read, Grep, Edit, and Read runs to split at tool-kind changes, and expects opening the first member body to dissolve the group. The archived client-surfaces contract and current Pager projection keep these activities in one group and preserve group expansion while a member body is open.

The 2.5.3 Core PTY run failed at `/tmp/grow-2.5.3-core-failed.log:20908`; the rendered header was `Read 5 files, Searched 2 patterns, Edited 1 file`, while the test waited for `Read 3 files, Searched 2 patterns`.

## What Changes

Update the existing PTY fixture comments and assertions to verify the archived continuous activity-group contract, retaining the end-to-end fold, expand, member-body open/close, refold, selection, and stable-row checks.

## Scope

Test-only. No product behavior or archived specification changes.
