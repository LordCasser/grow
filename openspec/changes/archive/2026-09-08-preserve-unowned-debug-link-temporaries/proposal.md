## Why
Debug latest-link update unconditionally removes its deterministic temporary path before symlink creation. This can delete an existing regular file or a competing update’s link.

## What Changes
Do not unlink before exclusive symlink creation. If creation fails leave existing temporary and latest entries unchanged. Keep cleanup after successful creation plus failed rename.
