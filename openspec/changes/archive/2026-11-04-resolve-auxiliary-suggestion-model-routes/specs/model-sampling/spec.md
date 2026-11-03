## ADDED Requirements

### Requirement: Auxiliary model selectors use their configured provider route

AI shell suggestion, prompt suggestion, and memory note rewrite SHALL resolve the selected auxiliary model through its exact configured catalog ID or alias. The resolved sampler configuration SHALL determine the sampling client and the provider-facing `ConversationRequest.model`, so endpoint, backend, credentials, and wire model belong to the same catalog entry. A wire-model string alone SHALL NOT be treated as a catalog identity. If a selected ID is unavailable, the caller SHALL follow its existing empty/failed result path without issuing a request through the active session route.

#### Scenario: Auxiliary catalog route differs from the session route

- **WHEN** an auxiliary entry point selects a catalog ID whose endpoint, backend, credentials, and provider-facing model differ from the active session
- **THEN** the actual request uses that catalog entry's full route and wire model, and is not sent to the active session provider.

#### Scenario: Provider-facing name is not a catalog key

- **WHEN** a selector contains only a provider-facing model name that is not itself a catalog key
- **THEN** Grow does not infer its provider or reuse the active session route, and the suggestion/rewrite follows its existing unavailable-model result.

#### Scenario: Selected model is unavailable

- **WHEN** AI suggestion, prompt suggestion, or memory note rewrite selects an ID absent from the usable catalog or without usable credentials
- **THEN** no provider request is sent through the active session route; the call follows its existing best-effort empty result or error path.

#### Scenario: Workflow Run selector is frozen

- **WHEN** one of these auxiliary calls runs under a Workflow Run with a frozen sampler snapshot
- **THEN** the exact Run image-description selector uses its dedicated frozen image sampler, another selector resolves only through a sampler captured by that Run with credential-only refresh, and a missing/changed selector fails without consulting live catalog routing.
