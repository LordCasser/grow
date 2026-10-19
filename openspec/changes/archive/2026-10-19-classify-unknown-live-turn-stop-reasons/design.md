# Design

The durable terminal rail remains first-wins and prompt-id gated. Classify its stop reason before entering the shared teardown: ACP success reasons (`end_turn`, `max_tokens`, `max_turn_requests`, and `refusal`) retain successful completion behavior; `cancelled`, `error`, and the existing `rate_limit` UX retain their current handling. Any other value, including a missing reason, is an error-class terminal with an explanatory failure marker and no success notification. This does not alter stale/duplicate terminal handling or late PromptResponse metadata merging.

Verification focuses on known success, unknown/interrupted classification, and duplicate terminal idempotency.
