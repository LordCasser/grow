## Design

Use the existing `resolve_aux_sampler_config` path for all three auxiliary entry points. The resolver already maps an exact catalog ID to endpoint, backend, provider credentials, wire model, and model sampling settings. Construct `SamplingClient` from this one result and put its `model` in the `ConversationRequest`; use the same client's backend for Sideband route and attempt metadata. Continue stamping only the existing session-local fields (service bearer resolver where its host gate permits it, and retry policy).

For Workflow Run sessions, preserve the run-owned frozen route. An exact match for the Run's image-description selector uses `image_description_sampler_for`; every other selector uses `sampler_for`, which only reads samplers captured by that Run and permits credential-only refresh. A failure in either selected branch is terminal for that route and does not fall through to the live catalog or active session.

Add real MockInferenceServer coverage with different server URLs, API backends, and API keys. Assert the received path, authorization, wire model, and that requests did not land on the primary session server. Also exercise unknown IDs to prove they do not fall back to the primary route.
