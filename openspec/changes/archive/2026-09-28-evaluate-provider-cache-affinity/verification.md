# Verification record

Status: theoretical review and offline mock complete under the user's revised acceptance scope. No real provider experiment has run.

## Prerequisite changes

The three prerequisite changes are archived and their verification records and task lists are complete in the current checkout:

| Prerequisite | Current state | Evidence |
| --- | --- | --- |
| `stabilize-image-budget-projection` | Archived; image, sampler and strict OpenSpec checks passed. | [verification](../2026-09-28-stabilize-image-budget-projection/verification.md) |
| `preserve-cache-usage-availability` | Archived; targeted five-crate tests and post-archive validation passed. | [verification](../2026-09-28-preserve-cache-usage-availability/verification.md) |
| `add-offline-prompt-cache-diagnostics` | Archived with `--skip-specs`; 15 offline fixtures and post-archive validation passed. | [verification](../2026-09-28-add-offline-prompt-cache-diagnostics/verification.md) |

The verified checkout has base commit `b38627016b411e40b5d98aee7a72dd18567e4452` plus uncommitted prerequisite changes. File digests identify the local experiment tools; this is not a provider-tested code revision:

- Offline diagnostics SHA-256: `fdce416f06d7968b5abff56b98c5d029d4e5fed98a0c4fe9b0a70cf0dc0260bc` (`scripts/analyze_prompt_cache.py`).
- Experiment harness SHA-256: `e44ac85be2e3831104ee4bbe46881382206ec9fa04a06a1034d08934252460fb`.
- Synthetic workload SHA-256: `1b08d38ae27addd5e43403bee756e3bba894d130a1dfb79a2e5c423b67f4299a`.
- Mock budget SHA-256: `0d504710eafcbcda25445df511f5cdd9958a4ba03135a14820df42699cab9bd3`.

## Route availability review

On 2026-09-28, local configuration was inspected for provider/model IDs, backend, and base URL host. Credential-bearing fields and secret values were skipped; no provider request was sent.

- A configured DeepSeek candidate exists: `deepseek/deepseek-v4-flash`, Responses backend, host `api.deepseek.com`. Official documentation compatibility with this exact model/backend combination remains unconfirmed.
- No direct OpenAI route was found. `openai-custom` uses Responses at `ai.moo.kim`; upstream provider/model identity and cache passthrough are unknown.
- No direct Anthropic route was found. A Messages route labelled `claude-opus-4-6-thinking` uses a local proxy; its upstream provider/deployment and cache passthrough are unknown.
- Other gateway routes are not counted as official-provider routes based on protocol compatibility or model naming alone.

The full candidate inventory and official cache-contract findings are in [route-matrix.md](route-matrix.md). Organization, workspace, deployment, and processing-region identities were not derived from secrets or account access; where unavailable they remain unknown.

## Theory and code review (2026-09-28)

Checked `chat-state/src/actor/request_builder.rs::prompt_cache_key` and its append/rewind/fork tests; `sampling-types/src/conversation.rs` conversions to Chat, Responses and Messages; `shell/src/session/actor/recap.rs` and its wire-shape test; the three archived prerequisite records; and the official sources linked from [route-matrix.md](route-matrix.md). The review found that the recap test's former name/comment claimed parent cache reuse although its assertions only proved a session-ID key and absence of parent tools. Its test name and that comment, plus the matching recap implementation comment, now describe only the verified local facts. Production request behavior is unchanged.

Official documents were checked again on 2026-09-28: [OpenAI Prompt Caching](https://developers.openai.com/api/docs/guides/prompt-caching), [Claude Prompt Caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching), [DeepSeek Context Caching](https://api-docs.deepseek.com/guides/kv_cache/), and the [DeepSeek Responses reference](https://api-docs.deepseek.com/api/create-response/). OpenAI's key and breakpoint roles vary by model generation; Claude documents content breakpoints and TTL; DeepSeek documents automatic caching, and its Responses reference lists model names different from the configured local alias. These sources support the conditional reasoning in [results.md](results.md), not a claim that the configured routes satisfy those contracts.

Reviewed every row of the earlier experiment matrix as a static mechanism question. The current key, breakpoint and recap behavior can be explained from code; provider hits, total cost, latency, TTL effects and route passthrough cannot be obtained from static review or mock. Decision: retain production cache behavior, correct the recap wording, and keep route-specific validation in [backlog](../../../backlog.md). No broad cache policy or provider control was introduced.

## Verification boundary

- Official documentation was consulted for OpenAI, Claude, and DeepSeek on the date above.
- Local route metadata review did not inspect or record API key values, environment variable values, auth-provider output, secret headers, or secret query parameters.
- No route was probed, no provider request was sent, no cost or cache usage was observed, and no result is presented as measured behavior.
- The user removed real requests and cross-provider measured conclusions from this change's acceptance scope. Route admission, live usage samples and numerical performance comparison remain unexecuted, and are not represented as completed by the theory review.

## Synthetic bounded harness (2026-09-28)

Added `harness/experiment_harness.py` with standard library only. Default mode is offline mock and loads only the adjacent synthetic fixture and fixed per-candidate budgets. The fixture carries synthetic evidence representations for all three prerequisites: image projection (including the complete selected/evicted choice), cache usage availability (including unknown write), and offline diagnostics (explicit request pair and comparison status). This validates evidence preservation and budget stopping only; it does not invoke the prerequisite implementations, inspect session data, or establish provider behavior.

Each mock route budget fixes `max_requests`, `max_output_tokens_per_request`, `max_total_output_tokens`, and `max_cost_usd`. Mock fixture costs are deliberately synthetic and do not represent pricing. Tests exercise request, per-request output, total output, and cost limits; missing prerequisites; explicit confirmed route config; and rejection of secret-bearing config fields.

There is no real-send mode. `--mode plan` requires an explicit route config with `confirmed: true` and all route identity and budget fields, then emits a bounded scenario/request plan with `network_requests_sent: 0`. It does not contact a provider or read credentials. This harness remains a bounded mock artifact; a real sender is outside the revised acceptance scope.

Commands and results:

- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s openspec/changes/evaluate-provider-cache-affinity/harness -p 'test_*.py'` — 6 tests passed again after the theory review.
- `PYTHONDONTWRITEBYTECODE=1 python3 openspec/changes/evaluate-provider-cache-affinity/harness/experiment_harness.py --mode mock` — completed; 0 network requests. The prior visible mock run recorded 2 DeepSeek-candidate, 2 OpenAI-candidate, and 1 Claude-candidate synthetic attempts within configured budgets.
- `cargo fmt --check --package shell`, `git diff --check`, and `openspec validate evaluate-provider-cache-affinity --strict --no-interactive` passed. Rust tests were not recompiled after changing only recap comments, a test function name and its assertion message; the earlier recap wire test still covers the same assertions, and no product logic changed.

Harness instructions: [harness/README.md](harness/README.md). No real provider experiment, route confirmation, price estimate, or measured provider cache conclusion was produced.

## Archive check

Archived with `openspec archive evaluate-provider-cache-affinity --yes --skip-specs` after all theoretical review tasks were checked. Post-archive `openspec validate --all --strict --no-interactive` passed (14 active specs); `openspec validate --archived --no-interactive` passed (548 archived changes). A local-link check over this archived change and `openspec/backlog.md` found no broken relative paths. The archive updated no product spec because request behavior did not change.
