# Record model route identity across cold session load

## Why

Timeline model changes validate the exact provider model and transport fingerprint, but cold load projects only the catalog model ID and reasoning effort. If an unchanged catalog ID now resolves to a different endpoint, backend, or wire model, the resumed actor silently uses the new route. Its next model transition records that route as `from_*`, breaking the durable chain and making the following load reject the session.

## What Changes

- Record a secret-free model route baseline during fresh session initialization, before the actor becomes available for prompts.
- During cold load, compare the last durable route identity with the exact current catalog route. Durably append one explicit route transition before publishing the actor when they differ. Preserve strict Timeline continuity validation.
- For sessions without any prior route observation, record the selected current route as a baseline without claiming to know the historical endpoint. Resident reconnect does not create a transition.
