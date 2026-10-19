## 1. Proposal and evidence

- [x] 1.1 Read behavior-goal requirements and the archived F1 audit evidence; record the existing durable/live snapshot divergence.
- [x] 1.2 Confirm this change's delta and design against the current coordinator and Control restore paths.

## 2. Implementation

- [x] 2.1 Construct a pending Behavior snapshot from the coordinator's existing selection semantics, preserving retained candidate and submission counter while keeping Plan runtime reset.
- [x] 2.2 Persist before applying the selection to live memory; preserve completed-plan clearing behavior.
- [x] 2.3 Promote both archived F1 probes into regression tests and cover durable failure and cold restore/new submission identity.
- [x] 2.4 Conduct an adversarial review of invalid runtime fields, completed references, and selection authority boundaries; record findings.

## 3. Validation and closeout

- [x] 3.1 Run rustfmt on the modified Shell files and check the diff for whitespace errors.
- [x] 3.2 Run applicable Shell tests; record the exact test names and results in verification.md.
- [x] 3.3 Run strict OpenSpec validation before archive; record post-archive validation in verification.md.
