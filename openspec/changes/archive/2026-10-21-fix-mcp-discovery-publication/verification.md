# 验证与对抗性复核

## 范围和证据
读取用户提供的 `grow-mcp-report-2026-10-21.md`。G1 从 doctor 的单次 list 调用确认；G2/H1 从后台捕获旧 ToolBridge 与 Agent rebuild 仅恢复当时 snapshot 的交错确认。隔离 HEAD 加新增回归（不加修复）稳定复现：Agent 切换后 bridge 工具为空，而 snapshot 有 demo__echo。此为实现级根因证据，不声称还原报告现场的全部执行轨迹。

工作区存在其他未提交的存储重构。初始工作区 Shell 测试编译遇到其中的 `RawValue::from_str` 错误，因此用 `git archive HEAD` 建立隔离源码，只叠加本次 MCP 修改；mod/spawn/support 逐处移植，未复制其他重构。隔离验证使用共享 target 加速；未覆盖或提交其他改动。

## 对抗性检查
- 分页：112 个工具、空页后仍有 cursor、后页错误、重复 cursor、跨页 cursor 环；错误不返回部分目录。
- 发布：真实 Agent rebuild 前后、owned/shared 发现阻塞时切换、退休 generation、禁用工具、过期 snapshot。
- 审查补漏：共享客户端发现也需协调当前 bridge；snapshot 必须携带原发起 generation；共享继承池发现后按当前 Agent 资格发布，避免继承范围变化导致遗漏。
- 失败路径：共享 list 错误不能当空目录；保留原完整目录，成功空目录/退休/资格撤销仍清理。
- 锁序：publication → McpState → bridge；远端握手/list 在 publication 锁外。生产 Agent 在 bridge 被共享前已激活资源域；测试夹具同步调整此顺序。
- 再次复核发现普通 snapshot wrapper 可与 detached Agent rebuild worker 并发；公共 wrapper 统一取锁，已持锁路径改用要求 MutexGuard 参数的 inner，避免旧 bridge 覆盖新快照及嵌套加锁。补充 publication boundary 回归。
- 工具未找到保持 NotFound 分类，仅报告当前会话注册事实；无工具执行重放或推测远端状态。

## 检查结果
- 隔离基线：仅加入 `mcp_initial_discovery_follows_agent_switch`，0 passed / 1 failed，断言为当前 bridge 空、snapshot 含 echo，证明测试抓到原故障。
- 最终 `cargo test --locked -p shell --lib mcp_ -- --test-threads=4`：150 passed / 0 failed，包括本次 7 个新增生命周期/边界测试和既有 MCP 测试。
- `cargo test --locked -p mcp --test tool_list_pagination -- --test-threads=2`：5 passed / 0 failed。
- `cargo test --locked -p tools --lib missing_mcp_tool_reports_local_registration_without_remote_claims`：1 passed / 0 failed。
- 最终 `cargo check --locked -p cli`：通过，默认生产依赖图；不是全 workspace 测试声明。
- `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：19 passed / 0 failed（归档前）。

所有网络测试仅使用本地 fixture；没有连接报告中的远端 MCP、修改其配置或替换已安装 Grow 二进制。

归档后全量 strict：18 passed / 0 failed。已删除本次隔离源码与日志明确标识的测试可执行文件（615.4 MiB）；其他任务仍在编译，未整体清理共享 target 或其缓存。
归档 tasks 校验 `openspec validate --archived --no-interactive`：683 passed / 0 failed。
