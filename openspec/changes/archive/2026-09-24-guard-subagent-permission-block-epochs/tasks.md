## 1. Epoch-safe mutation

- [x] 1.1 Restrict block member mutation to the Pager crate and reject appends/merges whose supplied source epoch differs from the block epoch; verify with focused block tests.
- [x] 1.2 Route normal append and reconnect merge through explicit epoch checks in `ScrollbackState`; verify same-epoch merge and cross-terminal separation with focused state tests.

## 2. Contract and validation

- [x] 2.1 Update the permission provenance backlog paragraph to record this closure and retain unrelated unresolved boundaries; verify the rest of the backlog is unchanged.
- [x] 2.2 Run focused Pager tests and `openspec validate --all --strict --no-interactive`; record results in `verification.md` before checking off completed tasks.
