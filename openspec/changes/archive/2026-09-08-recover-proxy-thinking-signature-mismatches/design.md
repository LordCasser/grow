# Design

Use the existing memory-only native lane and reset/rebuild loop. Extend rejection classification only for Serialization missing field `signature` when the failed request actually carried native continuation; after reset the portable request cannot qualify again. API 400 handling remains unchanged. No provider-name or model-family heuristics are needed.

Thinking start signature defaults to empty, while signature_delta remains strict. At response completion, any unsigned thinking block makes the entire native fragment unavailable; durable visible reasoning, text and tools are retained, and the next request uses portable projection. Signed responses retain the native lane. This avoids replaying unsigned blocks or inventing signatures.

Shell only admits a complete response and executes its tool calls after sampler collection succeeds; a failed parse cannot authorize the response tool calls. Existing historical tools remain portable facts. Model switch route_changed compares full model ID and already calls replace_sampling_route, including same-provider/different-model changes.
