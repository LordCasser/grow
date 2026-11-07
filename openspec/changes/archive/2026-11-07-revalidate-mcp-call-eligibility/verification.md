# Verification

日期：2026-11-07。范围：MCP-F1 —— 异步初始化/恢复后、发送 `tools/call` 前复核 captured client 资格。仅 `mcp` 发送 owner 与其近邻测试、Shell direct 参数适配。HEAD `8ab486a6`；工作树中的其余 dirty 属既有保留，本记录不据其取证。

## 实现契约（实际落盘）

`McpState::validate_tool_client(&self, server, tool, expected: &McpClient) -> Result<(), McpError>`（`servers.rs`，私有）按序判定：

1. 本地 `disabled_tools` → 拒绝；
2. `get_client` 必须仍是 captured 同 instance（比 `client_id`，进程内不复用）；
3. 选中 owned 时到此为止 —— 不新增“目录已发布”门，保留 direct 首次调用用途，且不受继承 ceiling 影响；
4. 选中 shared 且无同名 owned 覆盖时，先保本地捕获client身份，再沿 `SharedMcpEligibility` live chain 核 exact binding 与 qualified tool 资格；不以 child 尚未 reconcile 的 `shared_clients` map 代替实时上游资格。

两个入口复用同一 validator，且都在**最后一次异步准备之后、单次发送之前**取 state 锁判定、释放锁再发：

- 模型路径 `McpErasedTool::try_call_tool`：`ensure_initialized` 返回、`remaining` 非零后校验；
- direct 路径 `McpClient::call_tool`：签名加 `mcp_state: &Arc<Mutex<McpState>>`，`ensure_initialized` 后校验。Shell `extensions/mcp.rs::call_mcp_tool` 传已有 `mcp_state`，其 `disabled`/显式 `server_url` pre-admission 检查保持。

错误复用 `McpError::ClientError`；模型侧映射到既有 `process_manager` 类别，**不**称 outcome unknown，不因此 reset/reconnect/replay。锁等待与派发共用原 admitted absolute deadline：direct 路径改传 `call_tool_cancel_aware_until(mcp_service, params, deadline, timeout)`，不重造相对期限。

`call_tool_cancel_aware`（相对时间包装）在改后仅剩测试调用方，加 `#[cfg(test)]`；非测试构建不再有该 dead code。

## 旧红（原源码，未改发送链）

真实 in-process MCP server 经生产 ACP reverse bridge（`crate::acp_transport`）驱动；其 `initialize` 在 barrier 上扣留，测试在扣留期间撤销资格再释放。计数在 `tools/call` 分支**进入时**即记 `received`（参数校验与任何 delay 之前），答复产生后另记 `completed`，因此“未发送”与“已发但响应未完成”可区分。

`/tmp/grow-mcp-eligibility-oldred-1.log`：首条 model 用例在原源码上拿到 `Ok(ToolOutput::MCP(...OkayOutput("ok")...))` —— 初始化期间被 disabled 后旧 A 仍实际收到并答复 `tools/call`。这是发送链上的真实红。

`oldred-3.log`：2 passed / 7 failed。6 条为有效旧红（disabled、remove、same-name replace、ancestor revoke、ancestor rebind、recovery 来源）；2 条通过的是按预期必须绿的对照（survivor 单次派发、owned 覆盖不被连带拒）。

`oldred-3.log` 中 deadline 一项**不**计为产品红：当时 `fetch_add` 位于 delay 之后，已发未答被读成 0。该计数语义已按 review R2 修正，结论只在 green 轮成立。

`oldred-2.log` 为编译失败（`E0716` ×9，`tokio::pin!` 落在宏块内导致借用短于 future），`green-1.log`/`green-2.log` 亦为编译失败（新 direct 签名与既有测试未适配）。三者单列，不计产品缺陷，也不与行为红混算。

## 绿（最终源码）

`check-1.log`（scoped rustfmt 之后的最终文件状态）：`cargo +1.93.1 test -p mcp --lib --locked --offline eligibility_tests` → **16 passed, 0 failed**，exit 0。

model 9 条：

| 用例 | 证明 |
| --- | --- |
| `model_call_rejected_when_tool_disabled_during_initialization` | 扣留期间 disable → 明确拒绝，A received 0 |
| `model_call_rejected_when_server_removed_during_initialization` | 扣留期间 remove → 拒绝，A 0 |
| `model_call_rejected_without_falling_back_to_same_name_replacement` | same-name B 不被回落，A/B 均 0，B 未 init |
| `model_call_rejected_when_ancestor_revokes_tool_before_child_dispatch` | ancestor 撤 tool ceiling，child map 故意不 reconcile，仍拒 |
| `model_call_rejected_when_ancestor_rebinds_the_same_name_during_initialization` | ancestor 换同名 incarnation，旧 A 被拒，B 0 |
| `model_call_to_owned_override_survives_unrelated_ancestor_revocation` | owned 同名覆盖不被 parent revoke 连带拒，A 恰好 1 次，inherited 0 |
| `model_call_dispatches_once_when_eligibility_survives_initialization` | 无关 server 变化不误拒，A 恰好 1 次、不重放 |
| `model_call_does_not_refund_preparation_time_to_its_deadline` | 扣留消耗预算后不重给完整 timeout，received ≤1、不重握手 |
| `model_call_rejection_after_recovery_handshake_does_not_replay` | recovery 来源（第二次握手为 barrier），基线 (received,inits)=(1,2) 无增量 |

direct 7 条：上述前五项同语义（`direct_call_rejected_*` / `direct_call_dispatches_once_*`），加 `direct_call_does_not_refund_eligibility_lock_wait_to_the_send` —— 在初始化返回后扣留 session state 锁 600 ms，随后发送耗 700 ms，仍在 1 s admitted budget 内到期，`received=1`、elapsed < 1400 ms，证明资格锁等待未退还预算（对应 review R1）。

两种初始化来源各有实际 barrier 覆盖：首次初始化（`hold_init=1`）与 recovery（`hold_init=2`，`reset_transport` 后）；未将所有 client 直接置 Ready。

## 回归

- `regress-1.log`：`cargo +1.93.1 test -p mcp --lib --locked --offline` → **197 passed, 0 failed**，exit 0。含既有 cancel/timeout/no-replay/late-error replacement 定向用例。
- `shell-4.log`：`cargo +1.93.1 test -p shell --lib --locked --offline mcp_catalog_lifecycle_tests` → **13 passed, 0 failed**，exit 0（原 12 + 新增 `direct_call_rejects_a_stale_server_url_without_dispatching`）。
- 新增 Shell 控制经真实 `call_mcp_tool` 入口：过期 `server_url` 明确被拒（`endpoint no longer configured`）且 configured server received 0；随后传当前 endpoint 正常派发 1 次。非仅低层 helper。
- `shell-1.log` 报 `mcp` 的 `call_tool_cancel_aware` dead_code warning；加 `#[cfg(test)]` 后 `shell-2.log` 该 warning 消失，12 passed。`shell-2.log` 残留的 `irrefutable if let` 在 `shell/src/session/actor/input_admission.rs:2528`，属既有 dirty，不在本项范围。

证据时序披露：`green-6.log`（16）与 `regress-1.log`（197）早于 `#[cfg(test)]` 这次纯机械属性编辑。该属性只影响非测试构建，不改变单测语义；`shell-2.log`/`shell-3.log`/`shell-4.log` 在其之后，`check-1.log` 亦在最终（含 rustfmt 归一）源码上复跑 16 passed。

## 静态检查

- `rustfmt +1.93.1 --edition 2024 --check`：新增 `servers/eligibility_tests.rs` 干净；`extensions/mcp.rs` 干净；`mcp_catalog_lifecycle_tests.rs` 我新增的 hunk 干净。
- `servers.rs` 仍有 2 处 fmt diff（7073、7164），均为**既有测试代码**，非本次 hunk，未按整文件格式化处理。
- `git diff --check` 对三个被改文件：通过（exit 0）。
- `openspec validate --all --strict --no-interactive`：**22 passed, 0 failed**。

## 边界与未覆盖

- 单次请求准入后、网络中已发出的撤销仍按既有 originating-peer cancellation 与 uncertain outcome 处理；本项**不**承诺回滚，不新增自动 `tools/call`，不重放。
- 未做 mock 之外的远端生产服务、Windows 现场或全 workspace/CLI 回归；Shell 仅跑受影响模块。
- 未改：MCP SDK 注册 / reverse owner（F2 另项）、目录与供应商函数名上限、`rmcp` 版本 / vendor / manifest、重试政策、权限 owner、LSP/Skills/Pager/Goal 等既有 dirty。
- 未对历史旁支整支 cherry-pick；no-replay / 分页 / current-bridge 既有改动保持。
- `servers.rs` 中 `send_cancel_notification` / `MCP_CANCEL_NOTIFY_BUDGET` 为写入前的既有 dirty，非本项产出，未作为本项证据。

## 状态

实现与模块、Shell调用方里程碑均已通过父审查。父亲读最终diff、共同validator/两owner、真实旧红、最终16/197/13日志以及Shell显式endpoint拒绝和有效调用计数，R1/R2修订已闭合。原模型旧红已证明共同准备窗口的错误机制，direct路径在同一修复后的真实barrier矩阵与Shell调用方控制验收；没有为了再取得direct旧红而反复回退已修源码，不把direct新绿冒充旧实现失败记录。本记录不声称覆盖本项以外的远端生产或Windows现场。

归档已完成：`openspec archive revalidate-mcp-call-eligibility --yes` actual0，合入extension-runtime一条Requirement与五个场景；当时最后一个归档任务4/5提示保留，操作完成后勾选并复查。归档前strict **22/0**，归档后all strict **21/0**、archived strict **910/0**，命令均actual0，日志 `/tmp/grow-mcp-eligibility-{prearchive-strict,archive,postarchive-all,postarchive-archived}.log`。开发说明切换主规范，并把生产调用helper引用更新为absolute版本；backlog移除F1待修行，保闭合证据索引。MCP-F2注册owner及长名目录分别保独立change；未发布2.6.2。