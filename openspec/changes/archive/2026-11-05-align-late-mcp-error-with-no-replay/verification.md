# Verification

MCP-D1 的规范矛盾已核对。原 extension-runtime 中 `Late tool errors do not reset replacement services` 要求旧服务迟到错误使用新服务最多一次重试；同规范较新的 `Uncertain MCP tool calls are never automatically replayed` 明确副作用可能已发生、不得自动重放。采用后者既有契约，将迟到错误改为保留新服务并返回原调用错误。

父检查当前 `mcp/src/servers.rs::McpErasedTool::try_call_tool` 完整发送链：初始化沿同绝对 deadline，调用一次 `call_tool_cancel_aware_until`，错误只在原 client 仍 current 时尝试 reset originating service，不循环发送。取消守卫保存原 peer/requestId，替代服务不取得旧请求的取消或重发身份。

父读取已有公共发送函数回归 `late_tool_error_does_not_reset_already_recovered_service`：握手屏障后另一恢复建立新服务，再放行旧错误；断言原调用返回 Err、calls=1、inits=2，旧错误不额外握手。相邻 `retired_client_late_tool_error_does_not_recover_or_retry` 覆盖 remove/replace/disable，断言旧call=1、旧init=1，新endpoint call/init=0。以上是本轮静态审阅的现有测试断言；本次纯契约一致性修正未运行 Cargo，未声称新运行通过，也未修改任何业务代码。

delta 保留原 Requirement 名称与场景锚点，完整替换正文/THEN，和相邻 late timeout 及 replacement/annotation 规则一致。`docs/development.md` 更新同一契约链接和诊断解释，不另存第二份需求。主规范与docs现均为no-replay语义，MCP-D1已从backlog待修表移除并写入统一修复change的已验收证据。2026-11-05后续archive全量检查发现本归档遗漏两项收尾勾选；父重新核对实际产物后补齐，未新增Rust修改或运行测试。MCP-F1/F2/F3仍各自保留待修，尤其发送前post-initialization当前资格检查不由本次文字修改解决。
