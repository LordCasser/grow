## Implementation

- [x] Acquire the unified log inode's advisory lock around each append using the existing writer descriptor.
- [x] Add a deterministic trim/append interleaving regression and narrow the backlog item to its remaining long-record budget.

## Verification

- [x] Run diagnostics tests, formatting, diff and strict OpenSpec validation.
- [x] Archive the change and validate main and archived specs.
