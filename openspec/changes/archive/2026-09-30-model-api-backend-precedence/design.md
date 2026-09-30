## Existing path

`parse_provider_catalog` reads `[provider.<id>].api_backend` as a provider option and each `[provider.<id>.models.<model>].api_backend` as an optional model override. `ConfigModelOverride::with_provider_defaults` fills only an absent model value. `resolve_model_list` applies the resulting value to `ModelInfo`; `sampling_config_for_model` copies it to `SamplerConfig`; the sampler dispatches by that value. `ApiBackend::default()` is `chat_completions`.

The implementation already matches the requested precedence. Changing the resolver or adding a second setting would create two sources of truth. The gap is the documented contract and a test in which provider and model specify different protocols.

## Change

Specify the order as explicit model value, provider default, then `chat_completions`. Keep the provider setting at `[provider.<id>]` as the canonical user-facing form. Document that a model may also override `base_url` if a gateway exposes its protocol at a different URL. Add a test that resolves several models under one provider and checks the backend copied into each sampling configuration, including a model that explicitly chooses `chat_completions` against a different provider default.

## Verification

Run the focused shell test, formatting checks, and strict OpenSpec validation. Archive only after the scenario and verification record agree with the implementation.
