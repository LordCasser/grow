## Why

The release preflight's workspace-wide Rust formatting check reports two formatting differences: module declaration order in Pager and a wrapped constructor call in a workspace test. These are layout-only differences in otherwise complete work.

## What Changes

- Apply the workspace formatter to those two locations.
- Recheck formatting and whitespace before the feature commit.

No product contract changes. `skip_specs: true` keeps this formatting-only change from inventing a behavioral delta.
