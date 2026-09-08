# Verification

- Red: new preselected dispatcher regression failed on ordinary target 0 with changes only at later target 2 (0 passed / 1 failed, 0.08s).
- Green: pager dispatcher rewind group, 29 passed / 0 failed, 0.11s. New cases cover ordinary and inline preselection, unsorted later changes, no changes, target-inclusive changes, earlier-only changes, and missing preselection falling back to latest before computing range.
- Tests dispatch actions and inspect mode state, without executing backend effects or changing real files. No installed-binary UI verification.
- Linker emitted the existing large __eh_frame compact-unwind warning; tests exited successfully.
- Scoped rustfmt and diff whitespace checks, strict OpenSpec validation and cleanup recorded below.

Scoped rustfmt and git diff --check passed. Strict all validation passed 17/17 before archive. cargo clean removed 8,233 files / 3.5 GiB; 57 GiB available afterwards.
Post-archive strict validation: all 16/16, archived 259/259.
