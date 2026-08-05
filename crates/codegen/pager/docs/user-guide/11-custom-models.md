# LLM Providers and BYOK

The application has no bundled model, inference endpoint, or inference credential. Before the
first connection, configure at least one provider model and select a global default in
`~/.grow/config.toml`.

```toml
[models]
default = "deepseek/deepseek-chat"

[provider.deepseek]
api_backend = "chat_completions"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
env_key = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-chat]
name = "DeepSeek Chat"
context_window = 128000
```

`provider/model` is the stable catalog ID. The model table key is also the routing model sent to
the API unless its `model` field overrides that value.

## Architecture constraints

- `[provider.<id>]` owns the wire protocol.
- `[provider.<id>.options]` owns endpoint and credential settings shared by its models.
- `[provider.<id>.models.<model>]` owns model metadata and per-model overrides.
- `[models].default` seeds only newly created sessions.
- A session persists its last selected `provider/model`; reopening it restores that exact model.
- The provider-qualified catalog ID is distinct from the optional provider-facing `model` wire
  name; only the catalog ID may be persisted.
- Reasoning defaults resolve from narrowest to broadest: persisted session choice, model default,
  supported `[models].default_reasoning_effort`, then the model's lowest offered level.
- Changing a model inside one session never changes the global default or another session.
- Session persistence stores model IDs and session options, never provider secrets or endpoint
  snapshots.
- Remote model lists and compiled presets are not catalog sources.
- Product login credentials are not inference credentials.

These constraints keep provider configuration global while model selection remains session-local.

## API backends

`api_backend` selects the request protocol, not a vendor name:

| Value | Request path | Protocol |
|---|---|---|
| `chat_completions` | `/v1/chat/completions` | OpenAI-compatible Chat Completions |
| `responses` | `/v1/responses` | OpenAI-compatible Responses |
| `messages` | `/v1/messages` | Anthropic-compatible Messages |

Choose the protocol exposed by the endpoint. A Claude model served by an OpenAI-compatible gateway
still uses `chat_completions`; an Anthropic-compatible gateway uses `messages`.

## Provider options

Static keys are supported, but environment variables are preferred:

```toml
[provider.openai.options]
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
```

```toml
[provider.gateway.options]
base_url = "https://gateway.example/v1"
api_key = "sk-..."
```

Available shared options include:

- `base_url`
- `api_base_url`
- `api_key`
- `env_key` (a string or ordered array of environment-variable names)
- `extra_headers`
- `query_params`
- `env_http_headers`
- `auth_provider` or inline `auth`
- `context_window`

Endpoints that require no credential, such as a local Ollama-compatible server, may omit both
`api_key` and `env_key`.

### Command-backed BYOK

A provider can reference a local helper that returns a user-owned API key:

```toml
[provider.example]
api_backend = "responses"

[provider.example.options]
base_url = "https://api.example.com/v1"
auth_provider = "example-key"

[auth_provider.example-key]
command = "/usr/local/bin/read-example-key"
timeout_secs = 10
token_ttl_secs = 3600

[provider.example.models.model-a]
name = "Model A"
```

The helper prints a bare key or `{ "access_token": "...", "expires_in": 3600 }`. A configured
`api_key` or populated `env_key` takes precedence. Grow does not accept refresh tokens or own a
provider login lifecycle. Token cache expiry resolves in this order: helper `expires_in`, configured
`token_ttl_secs`, then a bare JWT's `exp` claim.

## Model options

```toml
[provider.local]
api_backend = "chat_completions"

[provider.local.options]
base_url = "http://localhost:11434/v1"

[provider.local.models.qwen3-coder]
name = "Qwen 3 Coder"
context_window = 131072
temperature = 0.2
output_limit = 8192
```

A model may override shared provider options when required. Common model fields include `model`,
`name`, `description`, `context_window`, `temperature`, `top_p`, `output_limit`,
`reasoning_effort`, `reasoning_efforts`, `extra_headers`, `query_params`, and
`env_http_headers`.

### Reasoning effort

Grow uses one internal set of effort values across providers: `none`, `minimal`, `low`, `medium`,
`high`, `xhigh`, and `max`. The selected API backend translates the session value to its wire
format: Chat Completions sends top-level `reasoning_effort`, Responses sends `reasoning.effort`,
and Messages uses its thinking/output-config fields.

Provider APIs do not expose a common model-capability discovery mechanism. Declare the exact
levels accepted by each configured model with `reasoning_efforts`; declaration order is also the
`Ctrl+X E` and `/effort` picker order. A table entry can provide a display label and mark the initial session
default:

```toml
[models]
default = "deepseek/deepseek-v4-pro"
default_reasoning_effort = "max"

[provider.deepseek]
api_backend = "chat_completions"

[provider.deepseek.options]
base_url = "https://api.deepseek.com/v1"
env_key = "DEEPSEEK_API_KEY"

[provider.deepseek.models.deepseek-v4-pro]
name = "DeepSeek V4 Pro"
context_window = 1048576
reasoning_efforts = [
  { value = "high", label = "High", default = true },
  { value = "max", label = "Max" },
]
```

Bare strings are accepted when labels and an explicit default are unnecessary:

```toml
reasoning_efforts = ["none", "high", "max"]
```

`reasoning_effort = "high"` sets a model default but does not by itself define a safe cycle menu.
Prefer `reasoning_efforts` for BYOK models: it derives support, and `default = true` marks the
model-specific default. That model default overrides `[models].default_reasoning_effort`. When no
model default is marked, Grow uses the global default only if the model lists it; otherwise it
leaves reasoning effort unset for the upstream service. `Ctrl+X E`, `/effort`, and `/model` all use
the same model-declared list. The selected value is stored with the session, so reopening a session
restores its last effort before any configured default is considered.

## Auxiliary models

Session summaries inherit the active session model when their setting is absent. Image reading is
different: when `image_description` is absent, images returned by `read_file` and rendered pages of
scanned or mixed PDFs remain multimodal tool-result content for the active model (text-layer PDFs
default to Markdown text extraction and never enter the image route). Setting `image_description`
routes that content to the configured auxiliary model and stores its textual description in model
context; resolution, transport, and empty-response failures are surfaced as text and never silently
fall back to another model. User message attachments and PDF `format="text"` / `format="markdown"`
are independent of this setting.

Auxiliary image-description requests do not hard-code `temperature`, `top_p`, or an output token
limit. Values configured on the selected model still apply; otherwise Grow omits them so the BYOK
service can select compatible defaults. This avoids provider errors from models that only accept a
particular temperature or reject unsupported sampling fields.

Prompt suggestions are disabled unless explicitly assigned a configured catalog model:

```toml
[models]
default = "primary/main"
session_summary = "fast/summary"
image_description = "vision/describe"
prompt_suggestion = "fast/suggest"
```

Every referenced value must be a configured `provider/model` ID.

## Missing or invalid configuration

Interactive startup validates the model catalog before authentication, model prefetch, or ACP
connection. When configuration is absent, it prints a provider-neutral template and offers to open
`~/.grow/config.toml` in `$VISUAL`, `$EDITOR`, or `vi`.

Non-interactive and stdio modes return an actionable error instead. They never synthesize a model or
connect to a bundled endpoint.
