## ADDED Requirements

### Requirement: Permission classifier details are bounded and complete

Permission classification SHALL use the full request detail only while evaluating that request. A classifier SHALL NOT issue an inference or produce an allowing verdict from a truncated request detail. Classifier detail SHALL be bounded to 1,024 bytes for MCP calls and 2,048 bytes for other access kinds. Classifier explanation text SHALL be capped at 240 bytes. If the complete detail exceeds its budget, classification SHALL return Unavailable before heuristic or model classification; request-local Auto authorization SHALL follow its existing unavailable fail-closed behavior. A completed permission audit event SHALL NOT retain raw access detail or model-generated classifier prose.

#### Scenario: Request detail exceeds classifier budget
- **WHEN** an Auto permission request contains access detail larger than the classifier's bounded input allowance
- **THEN** the classifier returns Unavailable without sending a partial detail for inference, and the request is not automatically authorized from incomplete evidence.

#### Scenario: Permission decision completes
- **WHEN** a permission request reaches an allow, deny, timeout, cancellation, or prompt outcome
- **THEN** the emitted audit event contains no raw access detail or untrusted classifier explanation, while the active request retains the exact input needed to make that decision until it resolves.
