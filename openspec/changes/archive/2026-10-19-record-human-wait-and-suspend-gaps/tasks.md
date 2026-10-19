## 1. Contract and schema

- [x] 1.1 Add Timeline typed interaction attempt events, validation/fold state, and exact-identity/duplicate/mismatch tests.
- [x] 1.2 Update developer architecture documentation to link the new durable wait and suspension activity contract.

## 2. Runtime boundaries

- [x] 2.1 Record question and Plan approval interactions at their actual reverse-request send/resolve boundaries, including all failure and suspension outcomes.
- [x] 2.2 Record human permission prompt boundaries at the actual Workspace ACP prompt call; automatic local decisions must not create human waits.
- [x] 2.3 Project complete interaction waits and matching runtime suspend/release offline intervals in activity; preserve unknown and overlapping activity conservatively.

## 3. Verification

- [x] 3.1 Add Timeline and activity tests for exact identity, retries/rebind, failure/unknown behavior, overlapping work, and suspend/release edge cases.
- [x] 3.2 Run focused checks and strict OpenSpec validation; record results without archiving until implementation and full acceptance pass.
