## Why

The prompt's footer always shows a human-readable model name. Users who operate the same model through multiple providers cannot see the selected route there, while users who prefer a compact footer should not have to show the provider.

## What Changes

- Add a persistent Settings choice for showing the provider in the prompt model label.
- Keep the existing compact `model (effort)` presentation as the default; when enabled, show the canonical `provider/model (effort)` route.
- Apply the choice immediately to session and Dashboard composers, including an opened child composer.
