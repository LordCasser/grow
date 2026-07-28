# Provider Authentication (BYOK)

Grow does not ship an inference account, model, endpoint, or credential. Authentication belongs to
each configured LLM provider in `~/.grow/config.toml`.

## Environment variable (recommended)

```toml
[provider.openai]
api_backend = "responses"

[provider.openai.options]
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"

[provider.openai.models.gpt-5]
name = "GPT-5"
```

```sh
export OPENAI_API_KEY="sk-..."
grow
```

`env_key` may also be an ordered list. Grow uses the first populated variable.

## Static key

A literal key is supported for controlled local environments:

```toml
[provider.gateway.options]
base_url = "https://gateway.example/v1"
api_key = "secret"
```

Prefer an environment variable so secrets are not stored in the configuration file.

## External auth helper

Providers whose bearer token rotates can reference an auth helper. The helper prints the token to
stdout; status messages may go to stderr. A provider key or populated `env_key` takes precedence over
the helper.

## OAuth

OAuth is also configured on a provider; Grow has no global inference login. The provider must
declare `type = "oauth"`, `issuer`, `client_id`, and scopes in its inline `auth` table (or reference
a named `[auth_provider.<name>]` table). Run `grow login <provider>` to authorize it and
`grow logout <provider>` to remove only that credential. Logging in never adds models or changes a
session's selected model.

## Local endpoints

An endpoint that requires no authentication may omit `api_key`, `env_key`, and auth helpers.

## Optional service and MCP authentication

Optional service endpoints have no compiled-in default and MCP OAuth remains server-scoped. Those
credentials authenticate only the explicitly configured service. They are never treated as an
inference credential for a BYOK provider.

See [LLM Providers and BYOK](11-custom-models.md) for the complete provider schema and protocol
selection rules.
