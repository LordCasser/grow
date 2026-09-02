# ROADMAP

> **定位**：本文件只记录长期计划，不是当前版本待办或实现授权。任何条目都不会自动进入实现；即使成熟度或验收条件已经满足，也必须由用户另行明确启动。

Grow 的配置保持本地化：全局配置位于 `$GROW_HOME/config.toml`，项目配置位于项目内的
`.grow/config.toml`。项目配置只影响当前项目，并按现有解析策略覆盖全局配置。

远程配置管理、deployment-config 服务、签名策略同步及其专用 CLI 不在规划范围内。

## 长期：MCP Elicitation 与交互式 MCP

> **状态**：等待协议与生态稳定；满足条件也不会自动进入实现，必须由用户手动启动。

MCP Elicitation 以及配套 UI、交互暂不绑定当前 draft wire。目标交互模型需要先进入有日期、非 draft 的正式 MCP 规范；官方 Rust SDK 和至少另一个主流官方 SDK 需要完整支持；生态中至少需要出现两个可互操作客户端和五个独立服务端，并且已有真实的 Form/URL 使用场景。这里写的是严格的成熟度证据清单，不是自动门禁。

未来如果启动，Grow 的边界固定为：

```text
版本化 MCP adapter
  → Grow PendingInteraction/UI
  → Timeline interaction request/outcome
  → 恢复原 MCP call
```

Elicitation 不是新 turn，不进入用户 FIFO，不建立第二条 interjection 通道，不采用上游 single-slot 覆盖模型。影响 MCP 调用是否继续的请求和结果属于 Timeline 事实；窗口焦点、开关等纯 UI 状态仍可临时保存。Form 不采集秘密，URL 模式要求显示服务端与完整目标地址、显式同意且禁止预取，秘密和令牌不得进入 Timeline 或模型 Surface。

参考：[MCP Elicitation draft](https://modelcontextprotocol.io/specification/draft/client/elicitation)、[2026 Release Candidate](https://blog.modelcontextprotocol.io/posts/2026-07-28-release-candidate/)。

## 长期观察：Worktree 生命周期

Worktree 生命周期、复用与安全边界继续等待上游稳定。它不是当前关键路径，不进入 v2.1.0 临时待办；后续仍需用户明确指定后才能启动。

## 搁置：底部 Status line

上游 `[ui.status_line]` 已确认是底部附加行：全屏模式位于 shortcuts bar 上方，minimal 模式位于输入框 info row 下方，不是 Grow 顶部的 `AgentStatusBar`。

这一方向搁置，不修改顶部状态栏、输入框下方区域或配置，不设计 builtin/command status line，也不进入 v2.1.0 临时待办。除非用户重新开启，否则不再推进。

## 长期：v2.1.0 架构审计后续

以下项目不是 v2.1.0 的实现内容。它们只记录已经确认的边界、影响与未来验收条件，不能作为当前代码的第二事实源。

- **LLM 非聚合失败的因果取证**：工具协议污染修复已保留聚合响应和 `IntegrityRepair` replacement，但这不等于覆盖解析失败、半截 SSE、空响应重试及所有 Sideband 失败中的原始证据。后续应沿现有 request/attempt/result 生命周期核对记录边界；凡参与重试、停止、计费或降级的部分输出和决定，都必须在所属 Timeline 中留有可验证的证据或不可变 artifact 引用。验收要求在流中断、解析错误、重试和崩溃窗口注入故障后仍能重建因果链，不能只保留成功 attempt 或另建调试日志充当事实源。此项与已实现的 Surface 工具身份/配对修复分开处理。

- **Workflow 进度 checkpoint**：Spawn seed 与 lifecycle 可以在 manifest 缺失时恢复 Run identity、冻结契约和终态，但不能完整重建 `current_phase`、Agent 行、累计预算等中间进度。未来应新增有界 Timeline checkpoint 或 journal fold，并证明 sidecar 全失时的投影与正常恢复一致。
- **Workflow Forgotten/tombstone**：当前 clear tombstone 仍位于 sidecar，历史 Timeline 不表达“该 Run 已被遗忘”。未来应设计 typed `Forgotten` 事实，并证明清理后重启不会复活旧 Run。
- **Workflow restore cap 顺序**：当前恢复数量上限先于全部有效性与 tombstone 判定。未来应先解析权威身份和清理事实，再对有效候选施加稳定上限，避免坏记录挤占额度。
- **Workflow sidecar repair**：seed fallback 目前只恢复内存投影，不把重建结果当作普通 CAS 写回。未来应提供独立 repair transaction，验证崩溃幂等性，且不能放宽正常 revision CAS。
- **Behavior projection freshness**：projection 与服务端 admission 已共用纯判定，但 foreground 变化后的 UI 广播仍可能短暂显示上一次快照。未来应在 admission facts 变化时按 revision 推送，并证明旧投影不能覆盖新状态。
- **Pager 图片草稿所有权**：v2.1.0 明确拒绝持久化临时图片路径。未来若需要图片恢复，必须设计独立 blob 生命周期、配额、加密/清理和引用完整性；验收要求崩溃后不泄漏路径、不悬挂 blob、不自动发送。
- **子 Agent follow-up admission**：当前只有创建 child 时的初始 `QueuePrompt`，没有父 Agent 向存活 child 追加消息的协议或工具。本版本不新增通道。未来若启动，必须把 ownership 与 liveness 分开判定，使用稳定 `message_id`，并且只在消息原子进入 child 自己的 Timeline/admission 后返回 `Accepted`；`Rejected` 与 `Unconfirmed` 必须分离，saturation、deadline、payload limit、channel closed 使用 typed outcome。实现不得增加全局通道、复用父 Agent FIFO，或把父 Timeline 当成 child 消息事实源。
- **Sandbox child FD inheritance**：v2.1.0 的 child network filter 封闭新建 socket、`sendmmsg` 与 `io_uring` 绕过，但 syscall filter 无法阻止 child 通过继承且已连接的描述符使用 `write`、`writev`、`sendfile` 或 `splice` 发送数据。未来应在进程创建边界建立显式 FD allowlist/close-range 契约，并以预连接 TCP/UDP 描述符做威胁回归；验收要求未知描述符在 exec 前关闭，确需继承的通道具有 typed ownership 与最小权限。
- **Folder Trust handle-relative 使用边界**：当前 schema、持久化 CAS 和进程内 cache 已绑定 root/`.git`/common-dir 的 filesystem identity，并在决策与 store read 后重验，但 loader 最终仍按 pathname 打开项目配置；目录可在最后一次 identity check 与实际读取/执行之间被替换。未来应让 trust admission 产出持有 OS directory handle 的 workspace capability，配置发现、读取与进程 cwd 全部相对该 handle 完成，或对每个读取结果做前后 identity 验证且把执行绑定到同一打开实体；验收要求在 check→open、open→parse、parse→spawn 三个窗口注入 rename/replacement 时均不得执行替换实体内容。
