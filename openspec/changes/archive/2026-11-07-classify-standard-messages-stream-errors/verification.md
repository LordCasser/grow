# Verification

Status: 父已核最终两处生产映射、实际reader/classifier与accounted loopback测试及有效old-red/53项green日志，接受此独立修复。最终发布仍须按统一版本完成集成；本记录不表示已在真实Anthropic部署复现。文档同步和OpenSpec归档由父完成。

## Scope and baseline

- Repository baseline: `8ab486a6e45870561b91dba872934a6e24a795f1` plus pre-existing dirty work. Before this task, `sampling-types/src/error.rs` already contained 271 added lines for error-envelope qualification; `sampler/src/client.rs` and `sampler/Cargo.toml` also contained unrelated normalization work. Those changes are preserved.
- Write scope: shared classification and its tests in `sampling-types/src/error.rs`; Messages reader/retry/actor consumption tests under `sampler/`; this verification record. No retry production policy, public error variant, source-envelope format, provider configuration, Pager/Shell/CLI or other change is modified.
- The fixed official-source basis is the archived [sources](../2026-11-06-compare-official-sampling-api-contracts/sources.md#anthropic-官方流错误和-sdk) and [analysis](../2026-11-06-compare-official-sampling-api-contracts/analysis.md#4-anthropic-messages主流式分工已覆盖标准错误类型需补核). This task does not reproduce a live Anthropic deployment or infer the user's route/version.

## Commands and evidence

All Cargo commands use `env -u CARGO_TARGET_DIR PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo +1.93.1`, with `--locked --offline`. Long commands run in the background with timeout disabled; actual process exits and full logs are recorded below. The default repository `target/` is used; no inherited target-directory override is accepted.

| Phase | Arguments after Cargo | Full log | Actual exit | Result |
| --- | --- | --- | --- | --- |
| Old red, helper | `test --locked --offline -p sampling-types --lib standard_messages -- --nocapture` | `/tmp/grow-standard-messages-oldred-types.log` | 101 | 3 passed, 1 failed: exact `api_error` still had `should_retry: Some(false)` |
| Old red, actual consumers | `test --locked --offline --no-fail-fast -p sampler --lib --test test_actor standard_messages -- --nocapture` | `/tmp/grow-standard-messages-oldred-consumers.log` | 101 | lib: 1 passed / 2 failed; actor: 0 passed / 3 failed. Reader/classifier observe the old veto. Both standard codes admit only 1 wire attempt instead of the expected bounded 2. `timeout_error` also retains old logical 500. |
| Green, shared error regressions | `test --locked --offline -p sampling-types --lib error::tests:: -- --nocapture` | `/tmp/grow-standard-messages-green-types.log` | 0 | 46 passed / 0 failed |
| Green, reader and classifier | `test --locked --offline -p sampler --lib standard_messages -- --nocapture` | `/tmp/grow-standard-messages-green-consumers.log` | 0 | 3 passed / 0 failed |
| Green, loopback | `test --locked --offline -p sampler --test test_actor standard_messages -- --nocapture` | `/tmp/grow-standard-messages-green-loopback.log` | 0 | 4 passed / 0 failed; includes 7 rejection modes inside the gate test |

Green total: **53 tests passed, 0 failed**. Background command `01a1b1b9-c1e7-73b3-9b28-6820b985e8e5` ran the three green commands sequentially and exited 0; each individual actual exit was also 0. No workspace/CLI/PTY suite or live provider call was run.

The earlier `/tmp/grow-standard-messages-oldred-sampler.log` command exited 101, but its first reader assertion incorrectly omitted the existing L2 `StreamStarted` event. That reader failure is **not** behavior-red evidence. The expectation was corrected before the valid consumers old-red command above; the classification remained unchanged until that valid old red completed. The same audit fixture review corrected the metadata assertion to use the existing `outcome.details.error` path before running the valid old red.

Static checks: `git diff --check` for the assigned files passed (actual exit 0). `rustfmt +1.93.1 --edition 2024 --config skip_children=true --check` passed for `error.rs`, `retry.rs`, and `tests/test_actor.rs` (actual exit 0). A full touched-file formatting check initially exited 1; newly added blocks were formatted. The pre-existing `client.rs` `response.doom_loop_check` concat layout was restored to preserve its owner's work, so this task does not claim that entire file passes rustfmt.

## Actual change

The production delta is confined to `sampling-types/src/error.rs:264,283`:

```diff
+        } else if normalized == "timeout_error" {
+            StatusCode::GATEWAY_TIMEOUT
         } else if normalized.contains("invalid")
@@
-            "server_error" | "internal_server_error" | "network_error" | "transient"
+            "api_error" | "server_error" | "internal_server_error" | "network_error" | "transient"
```

`api_error` keeps logical 500 and receives no unknown-code veto; `timeout_error` gets logical 504 and receives no veto. Neither sets `should_retry: Some(true)`. Sampler changes are exclusively test additions: real reader tests in `client.rs:4874,4959`, classifier tests in `retry.rs:600`, and existing loopback-harness consumers in `tests/test_actor.rs:1449,1698,1703,1708,1723`. The task adds 11 test functions and one local case helper, with no new recovery framework.

### Final tested source fingerprints

| File | SHA256 |
| --- | --- |
| `sampling-types/src/error.rs` | `1c89cc96def59468ac349a2d0fe858637fcf148e7014d45db26b9ab230441140` |
| `sampler/src/client.rs` | `777959386275e0897e7cc9876f537c67c57572a0c77ff84e03263ba6f1919abc` |
| `sampler/src/retry.rs` | `1eb3b9758003b31f4e02d63adeb120f84a369b0e90715109c0eb7aaa39e433a9` |
| `sampler/tests/test_actor.rs` | `133b79ac751d1c58cfd1755b134c90574b344b0fe912b34caeb471ea2f3c0b64` |

### Full-log fingerprints

| Log under `/tmp/` | SHA256 |
| --- | --- |
| `grow-standard-messages-oldred-types.log` | `36acdf6ed8eaef280485db950b0aae4c3664de615423fce4865e1a9623dfe1c3` |
| `grow-standard-messages-oldred-consumers.log` | `5df068bc0d9b827aa2d584ead962631826a3b1543e81a5e9c0b80a3b71338ebf` |
| `grow-standard-messages-green-types.log` | `086e4fd38220de860599db5392acafe994151f3c1de8f9716ba802d433979039` |
| `grow-standard-messages-green-consumers.log` | `7f5d6484c4ca90d51916d5c19b011dc34add2d4dfc8e59c6c5eac8127c83a205` |
| `grow-standard-messages-green-loopback.log` | `2a227a4b4f4aa99a7c99760d95190ea1300fa023325af08c8fc6132351a45903` |

## Identity evidence boundary

- Typed SSE tests preserve the original provider type/message and the local `SamplingEvent` request ID. That local ID is not a provider request ID.
- Named SSE tests preserve provider `request_id` through the existing named code/message parser.
- Raw attempt evidence retains the complete typed SSE bytes, including a supplied provider `request_id`. The existing typed parser does not structurally extract that top-level ID into `SamplingError`; this task deliberately preserves that existing parsing boundary.

## Acceptance coverage and limits

| Acceptance | Verified behavior |
| --- | --- |
| Exact standard classification | Current public helper recognizes normalized exact `api_error` as 500 with no unknown veto, and `timeout_error` as 504 with no veto; case/separator normalization preserves source spelling/message. Legal typed and named Messages SSE pass through the actual HTTP reader and L2; existing classifier permits the first bounded retry and rejects the cap. |
| Negative controls | `api_error_suffix`, `timeout_error_custom`, prefixed/lookalike codes remain unknown 500 with `Some(false)` at the helper. Mixed ordinary envelopes, duplicate declared type, malformed type and incomplete named errors remain non-retryable Serialization. Existing auth/quota/request/overload and envelope tests pass within the 46-test shared error set. |
| Recovery and vetoes | Classifier tests retain explicit `Some(false)`, context-length veto, disabled recovery and count caps. Loopback proves shared attempt exhaustion, disabled recovery, restored `replay_safe=false`, irreversible output, absolute deadline expiration during settlement, settlement failure and owner admission rejection all prevent another wire request. |
| Candidate rejection and readmission | Both codes first publish a valid tool candidate, then fail. `AttemptDiscarded(1)` follows its preview and precedes `AttemptStarted(2)`. Only the clean second response is accepted; failed tools never reach accepted-response projection. Repeated failures stop at two real wire attempts. The retry's request JSON equals the original valid request. |
| Attempt accounting | Every real failed Messages wire attempt reaches the existing usage owner as Incomplete with its unique `request:attempt` identity and original scope; a recovered final response settles Known 15 tokens exactly once. The next wire request observes preceding settlement, and a rejected settlement prevents readmission. Owner admission rejection may spend an internal recovery slot but produces no new provider wire attempt or fabricated charge. |
| Actual HTTP status | A real loopback HTTP 500 containing `timeout_error` remains 500; `x-should-retry:false` stops retries and the single actual attempt reaches Incomplete settlement. Public `from_api_response` also retains actual 500 both with and without the veto. |

The actor tests reuse the existing loopback fixture and real Messages reader; they do not own a tool executor. Candidate deltas, whole-attempt retraction, accepted-response tool projection and absence of failed-candidate admission are verified here. Actual Shell tool execution side effects are outside this sampler-only evidence. No independent production retry/recovery defect was exposed by these checks.

Cargo and all source/test write ownership were released after the three green commands actually exited. This verification record is the last coder write; documentation ownership is released with the Coder Result. No build/test command remains in flight. Pre-existing dirty work is preserved, including the unrelated sampler manifest and client normalization changes. No commit, archive, product release or real user configuration change was performed.

父已完成独立修复验收、开发说明和backlog同步。`openspec archive classify-standard-messages-stream-errors --yes`已将一个新增Requirement合入model-sampling并归档；归档后全量strict为22 passed / 0 failed，archive校验为903 passed / 0 failed，两个命令均实际exit0。日志为`/tmp/grow-standard-messages-{strict,archive,postarchive-strict,postarchive-archives}.log`。此本地归档不代表源码已提交或2.6.2已发布。
