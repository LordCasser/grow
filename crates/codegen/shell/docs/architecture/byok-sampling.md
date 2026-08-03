# BYOK Sampling Ownership

Sampling preferences belong to model configuration or the upstream LLM
service, not to individual shell features.

Internal requests such as image description, session title generation,
compaction, classifiers, memory rewriting, and prompt completion must leave
`temperature`, `top_p`, `max_output_tokens`, and `reasoning_effort` unset unless
the value is an unavoidable protocol requirement.

The sampler resolves optional values in this order:

1. An explicit request value, reserved for a real request-level contract.
2. The sampling client's `SamplerConfig` values.
3. The upstream service's defaults.

Protocol adapters may supply a value required by their wire format. For
example, the Messages adapter supplies `max_tokens` when the protocol requires
it and neither the request nor model configuration provides one.

Local safety controls such as input truncation, response sanitization, idle
timeouts, and wall-clock budgets are not sampling preferences and remain owned
by the feature that enforces them.
