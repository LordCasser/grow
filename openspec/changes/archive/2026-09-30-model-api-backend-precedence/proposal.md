## Why

One configured provider can expose models through different API protocols. Grow already resolves `api_backend` from a model override before the provider value, but the user guide describes the protocol as provider-owned and the canonical specification does not record this precedence. The existing tests do not distinguish an inherited backend from an explicit model override.

## What Changes

- Record the provider-default, model-override, and unset-backend resolution rule in `model-sampling`.
- Add a focused configuration-to-sampler regression for models using different API backends under one provider.
- Correct the configuration guide and show a mixed-protocol provider example.

This change formalizes and verifies the current runtime behavior; it does not add a configuration field or change request routing.
