## Context

The workspace permission manager holds typed AccessKind and classifier detail for the active decision. It currently clones raw detail and classifier output into PermissionEvent; Shell derives a safe durable summary but separately restores raw fields in the live update; Pager stores both variants without bounds. MCP and primary-context paths already have truncation helpers, but truncation does not prove the classifier saw a complete request.

## Goals / Non-Goals

**Goals:** Keep complete request evidence in the active decision path only; refuse an automatic judgment when its evidence exceeds the classifier input allowance; give live and replayed permission audit one safe projection.

**Non-Goals:** Change permission policy, expose detailed permission arguments in audit history, redact unrelated subagent descriptions, or add a generic secret-redaction framework.

## Decisions

- Before classification, check the complete detail against the established MCP and primary-context budgets. Oversized details produce Unavailable; the existing mode-specific fail-closed behavior decides the request. This avoids asking a model to authorize a truncated prefix.
- Remove raw access detail and classifier prose from PermissionEvent. Active prompt/UI request state remains the owner of raw details until the permission request resolves.
- Build the audit summary from stable tool identity and access kind, using fixed redaction labels for request payloads. Send the same summary in live and durable updates; keep classifier prose out of the client projection and child-facing denial explanation.
- Cap the structured classifier reason while parsing it, because it is diagnostic prose and has no authority over the verdict.

## Risks / Trade-offs

- Oversized Auto requests can no longer receive an automatic allow and may be denied or prompted according to the existing mode. This is deliberate because the classifier cannot safely decide from partial evidence.
- Permission audit details become less specific. The actual approval modal still presents the request while authorization is pending.

## Migration Plan

No data migration is needed. New durable permission events contain only the safe projection. Pager sanitizes legacy event detail fields at ingestion, so old stored records remain readable without restoring raw details to the UI.
