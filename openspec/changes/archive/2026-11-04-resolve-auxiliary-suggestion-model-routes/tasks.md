## Tasks

- [x] Read `model-sampling`, the A3/A4 route audit, `resolve_aux_sampler_config`, the three target entry points, and the existing selector docs.
- [x] Resolve the client and request model from one complete auxiliary sampler config at all three entry points.
- [x] Preserve Workflow Run's frozen sampler rules and keep unknown selected IDs from falling back to the session route.
- [x] Add mock transport tests that assert endpoint, backend, authentication, and provider-facing model for all three calls, plus unknown-selector failure.
- [x] Update the existing custom-model developer documentation to distinguish catalog IDs from provider-facing model names.
- [x] Align tool descriptions, selector documentation and release notes with catalog-ID routing; keep configuration-only auxiliary routing out of the model prompt.
- [x] Run targeted formatting/static checks and strict OpenSpec validation; record that Cargo validation is deferred to the parent coordinator.
- [x] Run the targeted Shell actor test on the coordinated Cargo artifact.
