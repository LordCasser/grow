## Why
Audit remaining JSONL maintenance helpers after migration to contained-directory and Timeline authority, including code with no callers. A nearby API comment still links to a removed rewind path API.

## What Changes
Record removal candidates for three private unused helpers and correct the obsolete rewind-loading documentation link. No functions, files or runtime features are deleted or enabled. No behavior contract changes; skip_specs is true because this change is source audit and documentation only.
