## 1. Snapshot transition

- [x] 1.1 Read the behavior-goal contract, existing coordinator transition, and archived F3 reproduction.
- [x] 1.2 Add a minimal spec-driven change with the Goal snapshot delta.
- [x] 1.3 Derive the target Goal snapshot by applying coordinator selection semantics to the current snapshot.

## 2. Regression coverage

- [x] 2.1 Cover deferred Plan preservation during first Goal activation and cold restore.
- [x] 2.2 Cover deferred Plan preservation during Goal definition reactivation and cold restore.
- [x] 2.3 Verify a Plan submitted after cold restore receives an identity greater than the persisted counter.
- [x] 2.4 Run the targeted Shell tests and strict OpenSpec validation.
