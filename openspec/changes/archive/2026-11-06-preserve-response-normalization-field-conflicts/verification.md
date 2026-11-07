# Verification: Responses usage completion preserves field conflicts

基线 HEAD `8ab486a6e45870561b91dba872934a6e24a795f1`，工作区含父/兄弟未提交改动（未触碰）。固定环境：`PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2`，`cargo +1.93.1`，`--locked --offline`，测试串行。所有 Cargo 调用均在本 change 的父释放窗口内执行，窗口已交还。

## 实际 red/green

### old-red（缺陷在旧实现上按设计复现）

命令：`cargo +1.93.1 test --locked --offline -p sampler --lib -- usage_detail_completion responses_usage_completion responses_conflicting_terminal --test-threads=1`
背景 task：`01a1af0d-2297-7f73-a695-a7a6d6312f11`，exit 101，25.91s（build 22.14s），**6 failed / 0 passed / 276 filtered out**。

关键失败（即缺陷证据，非测试问题）：

- `responses_usage_completion_never_admits_field_conflicts`：`duplicate_name_missing_details` 经真实 loopback HTTP/SSE → `SamplingClient::conversation_collect` → Responses L2 返回 `Ok(ConversationResponse)`，含 `tool_calls=[ToolCall{id:"call-direct", name:"test_tool", arguments:"{\"a\":1}"}]`、`provider_terminal=Responses{completed}`、`cache_read_known=false`——重复 function.name 因缺 usage 明细被 Value 重编码擦除后接纳为完成候选。
- `responses_conflicting_terminal_rejects_after_initial_function_call`：初始无 arguments 例外候选 + 重复 name 且缺明细的 terminal 同样被接纳。
- `usage_detail_completion_preserves_duplicate_field_rejection`：重复 `response.id` + 缺明细 → `decode_response_event` 返回 `Ok(Some(ResponseCompleted(...)))`。

### green（本修复实现，同一组测试）

同一命令，背景 task：`01a1af1a-d321-7593-b326-836ee1324098`，exit 0，15.75s，**6 passed / 0 failed / 276 filtered out**：

- `usage_detail_completion_preserves_duplicate_field_rejection`
- `usage_detail_completion_supplies_only_missing_details`
- `usage_detail_completion_does_not_repair_malformed_details`
- `usage_detail_completion_keeps_unknown_duplicate_extensions`
- `responses_usage_completion_never_admits_field_conflicts`
- `responses_conflicting_terminal_rejects_after_initial_function_call`

迭代中间轮（完整披露）：green 第 1 轮 `01a1af10-39af-75b2-957a-452fdf28dec4`（1/6，暴露成员字面量缺容器闭合 `}` 与 `Option<&RawValue>` 把显式 null 容器折叠为缺失两处实现缺陷）；第 2 轮 `01a1af14-8c73-73d3-934f-63450cd3438e`（4/6，暴露 `std::result::Result` 别名与测试 strip 模式多余引号、malformed 断言过严）。定位用单测探针 `01a1af19-cec7-71d3-82ce-3d0b613d999b`（临时 println，已移除）。父在预审与后续裁决中确认 `prepend_json_member` 缺闭合大括号、strip 测试改匹配实际插入布局、malformed 混合包不要求 helper 返回 None（验收为最终 SDK 拒收保真）。

### 既有回归（未删除任何失败断言）

命令：`cargo +1.93.1 test --locked --offline -p sampler --lib client::tests:: -- --test-threads=1`
背景 task：`01a1af1b-332b-7dc0-9fc5-0ba56ae8240e`，exit 0，0.86s，**69 passed / 0 failed / 213 filtered out**。包含既有 `responses_added_function_call_without_arguments_completes_with_later_fragments`、`empty_text_part_*`、`sparse_response_failure_*` / `responses_sparse_failed_*`、`function_call_start_preserves_other_sdk_decode_errors`、`empty_text_part_preserves_known_duplicate_field_rejection`、`sampling_auth_logs_omit_credentials` 等全部保留并通过。

## 覆盖对照（delta 场景 → 测试）

| Delta 场景 | 证据 |
| --- | --- |
| 合法缺明细 terminal 仍完成、availability 区分 | `supplies_only_missing_details`（typed 100/5/105、raw metadata 缺失=unknown；完整明细记 "0"）、L2 `positive_missing_details`（`cache_read_tokens()==None`）与 `positive_full_usage`（`Some(0)`） |
| 重复已知字段不因缺明细放行 | 单元 9 类矩阵（完整 vs 缺明细一致 `Serialization`，副本含重复原始字节）；L2 duplicate name/id 完整与缺明细 4 用例均拒收且错误含 `duplicate field` |
| 初始候选后 terminal 冲突仍拒收 | `responses_conflicting_terminal_rejects_after_initial_function_call`（含合法对照完成） |
| 补全资格限定对象形状（父 review 返工） | `usage_detail_completion_qualifies_only_object_containers`：完整序列形式 `[0]`×2 SDK 直接解码正控（decode accepted、helper None）；`[0]` + 另一明细缺失成对拒收（helper None、decode Serialization） |
| 明细 null/非对象不修 | `does_not_repair_malformed_details`（容器级 `null`/`7`/`"0"` → helper None + decode 拒收；count 级 null/`"0"`/`-1` → 副本保留原字节且 decode 拒收） |
| unknown 扩展重复键保持容许 | `keeps_unknown_duplicate_extensions`（顶层 `vendor_ext`×2、usage 内 `ext_metric`×2，补全后仍解码） |

## 其他核查

- `git diff --check -- crates/codegen/sampler/` 干净；diff 只含 `crates/codegen/sampler/Cargo.toml`（仅 `serde_json` 增 `raw_value` feature + 注释）与 `crates/codegen/sampler/src/client.rs`（+590/−22）。
- `openspec validate preserve-response-normalization-field-conflicts --strict --no-interactive` 通过（建立时与实现后各一次）。
- 源码 hash（交付态）：`client.rs` `551b6b8c2cad5c14e5763d8f013ec8e32bdd13ef1bb3fafb5ff3ac003c53d922`；`Cargo.toml` `029863740d734c5f764827ad9f187d38f1c85d79872bbcb5a2db4e6aca5a7a80`。
- 事实记录：serde 派生 struct 反序列化接受序列形式（`"input_tokens_details":[0]` 可解码），该形状属 SDK 既有容许而非被拒收形状，已从 malformed 用例移除并在测试注释说明；red 阶段该用例失败即此容许所致，非旧缺陷。

## 父 review 返工：补全资格限定对象形状（2026-11-06）

父 review 发现：`RawValue` 窄镜像的派生 struct 会接受序列形式（`input_tokens_details:[0]` → `cached_tokens` 判为已报告）。完整数组两容器都在时 SDK 直接解码本就通过，helper 不触发（不变）；但「数组 + 另一明细缺失」旧 Value helper 因 object-only 返回 None、保持缺失拒收，而镜像却把该数组视为已报告、补上另一容器后整体放行——超出本次「保留原始拒收」边界。

按父设计决定修正：本 helper 触及的 `usage` 文本与每个已存在明细容器的 `RawValue` 文本必须以 `{` 开始（`RawValue` 无外围空白且已良构，前缀判定精确），否则返回 None；保留 SDK 直接解码对完整序列形式的既有容许，不加递归 duplicate 规则；`MemberValue` 的 null/absent 区分不变。同步新增成对回归 `usage_detail_completion_qualifies_only_object_containers`，并把 delta 场景与 design 中「任何非对象都既有拒收」的过强说法改为 object-only 补全资格、SDK 原本可直接解码不改写。

**父级最终验证：object-gate + 新回归已通过。** 父在 task `01a1af2e-5317-7750-95c1-7426cdc33a70` 的第一条独立 Cargo 调用重跑 `cargo +1.93.1 test --locked --offline -p sampler --lib client::tests:: -- --test-threads=1`，实际 **70 passed / 0 failed / 213 filtered out**。该命令成功后才进入后续 Shell 命令（以 `&&` 串接），完整日志 `/tmp/grow-backlog-sampler-final.log`。最终源码 hash：`8fe47f919ab0cf6991c497f29a62bde1a76698a0aeebc6ee5729f4f668b57226`。父核对三处 object-only 资格门与仅插入、保留原始字节的实现，开发说明已同步。

## 覆盖边界与剩余限制

- 真实链路验证到 `conversation_collect` 返回的 L2 `ConversationResponse`（无 Completed 工具候选、错误保留 `duplicate field`）；shell Timeline 持久接纳、跨层 Sampler 里程碑集成与真实 provider 行为不在本项范围，由父在里程碑执行。
- `sampling-types/error.rs` 流错误预解析（backlog 第 2 项）与 readiness 项未触碰。
- 未运行 sampler crate 以外的 suite；`cargo build -p sampler`（lib 无 dev-deps）依赖 `raw_value` 显式 feature，本 change 已在 Cargo.toml 声明，不依赖传递 feature 统一。
