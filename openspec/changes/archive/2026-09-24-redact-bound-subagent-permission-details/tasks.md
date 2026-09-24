## 1. Classifier boundary

- [x] 1.1 Reject over-budget complete classifier detail as Unavailable before heuristic/model judgment; verify over-budget MCP and Bash inputs cannot produce an allow.
- [x] 1.2 Bound parsed classifier reason text; verify multi-byte and oversized model reasons stay within the declared byte budget.

## 2. Audit projection

- [x] 2.1 Stop PermissionEvent from retaining access detail and classifier prose; verify emitted events omit both while decisions still use full in-budget input.
- [x] 2.2 Make live and durable Shell projections share the same safe summary and generic decision reason; verify secret sentinels are absent from serialized live and durable updates.
- [x] 2.3 Bound Pager permission audit event text at its ingestion boundary; verify oversized externally supplied fields cannot expand retained or rendered details.

## 3. Verification and records

- [x] 3.1 Run focused workspace, Shell, and Pager tests plus formatting checks; record results in verification.md.
- [x] 3.2 Remove only the resolved access_detail/classifier_reason backlog clause and validate all OpenSpec changes before coordinated archive.
