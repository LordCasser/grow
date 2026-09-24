## 1. Contract and implementation

- [x] 1.1 Add the versioned availability contract and scenarios.
- [x] 1.2 Add a monotonic revision to Shell-built projections and reject stale revisions in Pager.
- [x] 1.3 Publish refreshed projections at prompt/Goal promotion, idle settlement, compaction release, and pending step-control admission/release.
- [x] 1.4 Keep command updates publishable when the current command list is empty.

## 2. Verification

- [x] 2.1 Add Shell prompt-promotion and return-to-idle regression tests.
- [x] 2.2 Extend the Pager open-settings test to cover out-of-order revisions.
- [x] 2.3 Run focused Shell and Pager tests and `git diff --check`.
- [x] 2.4 Run `openspec validate --all --strict --no-interactive` and record verification.
- [x] 2.5 Archive the change and run post-archive validation.
