# Provider cache route matrix

Status: official documentation, local route metadata, and code review only; no provider request has been sent. This matrix is a theory input, not an admitted-route test plan required for this change's completion.

Checked on 2026-09-28 against the official documentation linked below. Provider documentation describes provider-level behavior. It does not establish that a compatible gateway forwards the same fields or that a particular configured model/deployment supports them.

## Route inventory

The local user configuration was inspected for provider IDs, model IDs, API backend, and base URL host only. Credential and tenant values (`api_key`, `env_key`, auth provider, headers, and query parameters) were skipped and are not recorded here. These are local runtime routes, not committed project defaults.

| Candidate | Local non-secret route metadata | Official cache evidence | Applicability / unresolved facts |
| --- | --- | --- | --- |
| DeepSeek `deepseek/deepseek-v4-flash` | Provider `deepseek`; backend `responses`; host `api.deepseek.com`; configured default model. | DeepSeek documents automatic disk context caching and Chat usage hit/miss fields. Its [Responses API reference](https://api-docs.deepseek.com/api/create-response/) lists `deepseek-flash` / `deepseek-v4-pro` and `input_tokens_details.cached_tokens`. See also [Context Caching](https://api-docs.deepseek.com/guides/kv_cache/). | Not executed. The configured model alias is not one of the reference's listed wire names; its mapping/support remains unconfirmed. The [Responses guide](https://api-docs.deepseek.com/guides/responses_api/) says cache key/retention parameters are unsupported; no client breakpoint or TTL control is established. |
| OpenAI direct | No direct OpenAI provider route was found in the inspected local configuration. | OpenAI documents model-dependent behavior. For GPT-5.6 and later, `prompt_cache_options.ttl` supports `30m`; explicit breakpoints and optional `prompt_cache_key` are available, with the key documented for accounting separation. Earlier models use `prompt_cache_retention` according to model support, and stable `prompt_cache_key` can help routing affinity. Cache entries are machine-local, not shared across organizations or regional processing boundaries, and hits are not guaranteed. See [OpenAI Prompt Caching](https://developers.openai.com/api/docs/guides/prompt-caching). | No direct route to execute. Exact model, endpoint, organization retention mode, and processing region are unknown. Do not apply one model generation's key, retention, or breakpoint rules to another. |
| OpenAI-compatible gateway candidate | Provider `openai-custom`; backend `responses`; host `ai.moo.kim`. | No official evidence establishes this host's upstream provider, model deployment, or cache-field passthrough. OpenAI protocol compatibility alone does not establish OpenAI cache semantics. | Not eligible for the official OpenAI row. Could be evaluated separately only after upstream identity, deployment, and field passthrough are established. |
| Anthropic Claude direct | No direct Anthropic provider route was found in the inspected local configuration. | Claude Messages documents automatic or explicit `cache_control` breakpoints; `ephemeral` TTL defaults to `5m`, with `1h` also supported. Cache use refreshes the lifetime. The prompt supports up to four explicit breakpoints and applies a 20-block lookback. The Messages API reference exposes `cache_control` and TTL values `5m` / `1h`; no `prompt_cache_key` request field is documented. See [Claude Prompt Caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching) and [Messages API reference](https://platform.claude.com/docs/en/api/http/messages). | No direct route to execute. Exact model/deployment and workspace identity are unknown. |
| Claude-labelled Messages proxy candidate | Provider `proxy`; backend `messages`; host `127.0.0.1`; configured model ID `claude-opus-4-6-thinking`. | The local route metadata does not identify the proxy's upstream provider, deployment, or whether it forwards `cache_control` and usage fields. A Claude-like model ID and Messages backend do not prove the Anthropic contract applies. | Not eligible for the official Anthropic row until upstream identity and passthrough are confirmed. |
| Other Messages gateway candidate | Provider `opencode-messages`; backend `messages`; host `opencode.ai`; configured model `minimax-m3`. | No evidence ties this route to Anthropic's cache contract. | Keep separate from Claude; no provider request sent. |

## Official contract facts

### OpenAI

- Caching is automatic for supported models, while cache lifetime, minimum cacheable prefix, usage rounding, pricing, and breakpoint behavior vary by model generation.
- GPT-5.6 and later document `prompt_cache_options.ttl="30m"`, explicit breakpoint controls, and optional `prompt_cache_key`. On these models the key is not needed to optimize routing and can be used to separate cache accounting.
- Earlier model generations document `prompt_cache_retention` values according to the model, and a stable `prompt_cache_key` can help route related requests. Keys influence routing; they do not pin a machine or guarantee a hit.
- Caches are machine-local and are not shared across organizations or regional processing boundaries. A route matrix therefore needs the exact model and processing boundary; a generic “OpenAI” row is insufficient for an experiment.

### Anthropic Claude

- Messages supports cache breakpoints using `cache_control` with `type="ephemeral"`; the TTL defaults to `5m`, with `1h` available where supported.
- The TTL is refreshed when cached content is used. Up to four explicit block-level breakpoints are supported, with a 20-block lookback rule.
- The documented request surface uses breakpoint controls rather than a prompt cache key. These controls and usage fields must be verified through any gateway before applying the direct Claude API contract to it.

### DeepSeek

- The API documentation describes automatic disk context caching and best-effort prefix reuse; it does not describe a caller-selected cache key, retention parameter, or explicit breakpoint.
- Chat documentation reports `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens`; the Responses reference reports `input_tokens_details.cached_tokens`. Do not assume the local alias or gateway returns both shapes.
- The docs say unused cache is automatically cleared, usually within a few hours to a few days. This is not a guaranteed TTL or retention control.
- Cache construction takes seconds, so immediate follow-up behavior should not be treated as proof that a cache entry has or has not been persisted.

## Fields for future route-specific validation

Before execution, record for each route: canonical provider identity; exact requested and served model/deployment identifier (if response evidence supplies one); backend and endpoint identity; deployment/region and non-secret tenant-isolation label where known; SDK/gateway version; check date; supported usage fields; and the official source. Do not infer provider identity from backend name, URL path, or model label. Do not record credential or secret header/query values. Unknown identity or passthrough remains a blocker, not an invitation to probe automatically.
