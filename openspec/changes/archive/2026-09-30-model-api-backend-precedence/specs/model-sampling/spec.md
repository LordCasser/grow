## ADDED Requirements

### Requirement: Model API backend overrides its provider default

Grow SHALL select a configured model's API backend from its explicit `api_backend` when present, otherwise from its provider's `api_backend` default, otherwise from `chat_completions`. The resolved backend SHALL be carried into that model's sampling configuration and used to choose its request protocol. Models within the same provider MAY use different backends.

#### Scenario: Model inherits provider backend

- **WHEN** a provider sets `api_backend = "responses"` and one of its models omits `api_backend`
- **THEN** that model's sampling configuration uses Responses.

#### Scenario: Model overrides provider backend

- **WHEN** a provider sets `api_backend = "responses"` and another model explicitly sets `api_backend = "messages"` or `"chat_completions"`
- **THEN** that model's sampling configuration uses its explicit backend, independently of the first model.

#### Scenario: Neither level specifies a backend

- **WHEN** a provider and its model both omit `api_backend`
- **THEN** that model's sampling configuration uses Chat Completions.
