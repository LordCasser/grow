# Verification: leader ingress connection identity (MCP-F2 increment 1)

## Scope

生产落点仅 `crates/codegen/shell/src/leader/server.rs`：

- `inject_capabilities_into_session_new` 对有效 `session/new`、`session/load` 在 `params._meta` 无条件写实际 accepted `ClientId.0` 为 `grow/leaderClientId`，覆盖 caller 值（含错误类型）。
- `_meta` absent 或 `null` 先物化为 object 再注入；非 object 非 null 不被物化。
- `has_malformed_session_meta` + ACP ingress 早拒绝：`session/new`/`session/load` 的 `params` object 中 `_meta` 存在且既非 `null` 也非 object 时，在任何 subscriber/driver/backfill/pending request/load buffer 写入前，向 origin connection 回 JSON-RPC `-32602`（原 `id`），不转发 `acp_tx`；无 `id` 的畸形 notification 直接丢弃。
- 未改 `extract_target_client_id`、target 单播、断线丢弃、普通 reverse driver 政策；未改 ACP schema/vendor/MCP server/SDK producer。

`server.rs` 在工作区原本已有其它 dirty hunk（例如 `interaction_inner_params`、`live_descendant_sessions` 及早前测试增量）；本增量未修改它们。本增量自己的区域见文末 Diff boundary。

## Typed ACP 事实（修正原先假设）

`agent-client-protocol-schema 1.5.0` 的 `NewSessionRequest`/`LoadSessionRequest` 对 `_meta` 使用 `#[serde_as(deserialize_as = "DefaultOnError")] meta: Option<Meta>`。用字段完整的 fixture（`cwd`、`mcpServers: []`，load 另有 `sessionId`）验证：非 object 的 `_meta` 经 typed 反序列化得到 `Ok(None)`，不是 `Err`——ACP 层会把畸形 metadata 静默当作 absent。

因此“交原 ACP 反序列化拒绝”不成立；若不早拒绝，session 会在没有可信 owner 的情况下创建。修正后的入口合同是 leader 自己拒绝（`-32602`）。首轮 green（task `01a1b309-35ac-7732-a206-9ccbc960ee06`，log `/tmp/grow-sdk-owner-ingress-green.log`）中 `typed.is_err()` 的通过是缺 `mcpServers` 造成的假证；该断言已被完整 fixture + `Ok(None)` 断言替换，首轮 green 不作为本项最终证据。同一遗漏也出现在真实 ingress 用例首版 params（缺 `mcpServers: []`，只证明 raw forward）；已补 `[]` 并对实际 forwarded params 增加 typed parse 成功断言，按 exact filter 重跑通过。

## Evidence

固定环境（所有 Cargo 命令相同）：

```
env -u CARGO_TARGET_DIR -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC \
  -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 \
  CARGO_BUILD_JOBS=2 cargo +1.93.1 test -p shell --lib --locked --offline <filter>
```

### 1. 旧源码最小红：actual exit 101，断言失败而非编译错误

- task `01a1b304-3df2-7972-9cf1-35babbde82af`，log `/tmp/grow-sdk-owner-ingress-oldred.log`。
- filter `inject_capabilities_overwrites_forged_leader_client_id`；`Some(7)` vs `Some(42)`，`0 passed; 1 failed; 4765 filtered out`。
- 当时生产代码仍是“缺失才补值”；测试先改预期取得有效 red，之后才改入口。

### 2. 修订后联合运行：actual exit 0

同一后台任务依次运行以下 filter，追加到 `/tmp/grow-sdk-owner-ingress-regress.log`；task `01a1b30d-06d6-7a43-8d0b-6d0086a4c44d`，exit 0：

| filter | result |
| --- | --- |
| `malformed_session_meta` | 2 passed |
| `forged_leader_client_id` | 3 passed |
| `inject_capabilities` | 26 passed |
| `extract_target_client_id` | 2 passed |
| `leader_client_id_` | 5 passed |
| `two_clients_one_session_broadcast_and_driver` | 1 passed |
| `fallback_routing_forwards_notifications_but_drops_responses` | 1 passed |
| `accepted_sampling_candidate_flushes_pending_load_once` | 1 passed |
| `live_broadcast_during_load_is_buffered_then_flushed_after_response` | 1 passed |
| `driver_only_request_during_sampling_candidate_is_not_buffered` | 1 passed |
| `exit_scope_waits_for_new_and_load_request_registration` | 1 passed |

无 failure；编译告警只有工作区既有的 `input_admission.rs` irrefutable `if let`。不同 filter 之间有重叠用例，上表按实际每轮披露，未把 11 轮命中数合计成唯一案例数。

决定性 socket 证据：

- `session_new_and_load_overwrite_forged_leader_client_id_at_ingress`：`setup_persistent_server_with_agent` + 两次 `connect_register_get_id` 建真实 accepted connection（forged target 与 actual owner，断言 id 不同）。同一 actual connection 发带 `_meta.grow/leaderClientId = forged` 的 `session/new`、`session/load`；输入 params 含 v1 必需 `cwd`/`mcpServers: []`（load 另有 `sessionId`），实际 `acp_rx` forward payload 的该字段均为 actual id，不是 forged。随后把实际 forwarded `params` 用对应 `acp::NewSessionRequest`/`LoadSessionRequest` typed parse：解析成功且 `_meta.grow/leaderClientId` 为 actual id，排除“缺字段导致 raw 断言通过”的假证。load 响应完成后再发带 actual id 的 replay，只有 actual loader 收到，forged target 收不到。补 `mcpServers` 后仅以 exact filter 重跑一次：task `01a1b312-7e35-7c10-98cf-87987afe5231`，exit 0，`1 passed; 0 failed`，追加于同一 regress log。
- `malformed_session_meta_is_rejected_at_ingress_without_forwarding`：真实 socket 依次发畸形 `session/new`(id 81) 与 `session/load`(id 82)，origin 分别收到 `error.code = -32602` 且 `id` 为原值；`acp_rx` 无对应请求。随后有效 `session/new` 仍被转发且携带 actual id（因果屏障，证明拒绝未留下 pending/load buffer）。无 `id` 的畸形 `session/load` notification 无回复，且下一个 `acp_rx` 元素是随后有效请求（证明它被丢弃而非先转发）。对 `sess-rejected` 发 reverse request 不被任何 connection 收到（证明未留下 subscriber/driver 路由）。
- helper 证据：`inject_capabilities_overwrites_forged_leader_client_id` 覆盖 new+load × 数值/字符串伪造值；`inject_capabilities_materializes_absent_or_null_meta_with_leader_client_id` 覆盖 absent/null 物化并注入；`inject_capabilities_leaves_non_object_meta_for_ingress_rejection` 证明 helper 不物化、不改 payload，且完整 fixture 下 typed 为 `Ok(None)`。

### 3. 既有路由回归（含于上表）

`extract_target_client_id` 两例、`leader_client_id_unicasts_to_target_only`、`leader_client_id_dropped_when_target_disconnected`、`two_clients_one_session_broadcast_and_driver`、`fallback_routing_forwards_notifications_but_drops_responses`、load replay/load buffer 两例、`driver_only_request_during_sampling_candidate_is_not_buffered`、`exit_scope_waits_for_new_and_load_request_registration` 均通过；普通 reverse/driver 政策未改。

### 4. Formatting

```
rustfmt +1.93.1 --edition 2024 --check --config skip_children=true \
  crates/codegen/shell/src/leader/server.rs
```

exit 0，空日志 `/tmp/grow-sdk-owner-ingress-rustfmt.log`；没有改写整文件。

`openspec validate --all --strict --no-interactive` exit 0，22 passed / 0 failed（含本 change；父新加的 AC07 设计已计入最新实际数量），log `/tmp/grow-sdk-owner-ingress-openspec.log`。

`cargo check` 按协调方指示未单独运行，避免重复同一 lib-test 编译；上述 `--lib` test 构建即本改动的编译证据。

## Coverage and limits

- 已覆盖：真实 leader accepted connection 的 new/load forward payload；伪造 owner（含错误类型）；absent/null；畸形 metadata 的显式拒绝/notification 丢弃；拒绝后无 session 路由；target 单播与断线不 fallback；load replay 目标；generic driver/fallback 与 load buffering 回归。
- 未覆盖/不在本增量：SDK producer、`GatewayAcpInvoker`、spawn/child/resident/cold 注册绑定（下一增量）；ACP schema 变更；`leader_soak`/全 workspace；Windows 或外部部署；真实 SDK initialize/list/call 端到端（属增量 2 里程碑）。
- load replay 证据是“ingress 实际注入 id + 现有 replay 机制按该 id 单播”的组合，不是真实 SDK producer 的端到端。

## Diff boundary

`crates/codegen/shell/src/leader/server.rs`：

- `has_malformed_session_meta`（`is_session_load_request` 后）
- `inject_capabilities_into_session_new`（doc、`_meta` 物化、无条件 owner 覆盖）
- `run_leader_server` 的 ACP ingress 早拒绝分支
- tests：`inject_capabilities_overwrites_forged_leader_client_id`（改名并扩展）、`inject_capabilities_materializes_absent_or_null_meta_with_leader_client_id`、`inject_capabilities_leaves_non_object_meta_for_ingress_rejection`、`malformed_session_meta_detection_accepts_optional_and_other_shapes`、`session_new_and_load_overwrite_forged_leader_client_id_at_ingress`、`malformed_session_meta_is_rejected_at_ingress_without_forwarding`
- tests 模块局部 import `agent_client_protocol::schema::v1 as acp`

本增量只新增本验证记录，未改 design/proposal/tasks/spec/backlog。源码、Cargo 和文档均未提交、未推送、未归档、未发布。
