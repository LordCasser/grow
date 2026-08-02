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

## Local key helper

Providers may reference a local helper that reads a user-owned key. The helper prints either a bare
key or `{ "access_token": "...", "expires_in": 3600 }` to stdout; status messages may go to
stderr. A provider key or populated `env_key` takes precedence over the helper. Grow never accepts a
refresh token from the helper. For cache expiry, `expires_in` takes precedence over the configured
`token_ttl_secs`, which takes precedence over a bare JWT's `exp` claim.

## No login lifecycle

Grow is BYOK-only. It does not implement OAuth, OIDC, device login, browser callbacks, credential
refresh, or `login/logout` commands. Key rotation belongs to the environment, configuration, or
local helper that owns the key.

## Local endpoints

An endpoint that requires no authentication may omit `api_key`, `env_key`, and auth helpers.

## MCP authentication

Remote MCP servers use explicitly configured headers or `bearer_token_env_var`. Grow does not
discover or perform MCP OAuth.

See [LLM Providers and BYOK](11-custom-models.md) for the complete provider schema and protocol
selection rules.
