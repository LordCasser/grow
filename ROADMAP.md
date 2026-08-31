# ROADMAP

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

- **Workflow 进度 checkpoint**：Spawn seed 与 lifecycle 可以在 manifest 缺失时恢复 Run identity、冻结契约和终态，但不能完整重建 `current_phase`、Agent 行、累计预算等中间进度。未来应新增有界 Timeline checkpoint 或 journal fold，并证明 sidecar 全失时的投影与正常恢复一致。
- **Workflow Forgotten/tombstone**：当前 clear tombstone 仍位于 sidecar，历史 Timeline 不表达“该 Run 已被遗忘”。未来应设计 typed `Forgotten` 事实，并证明清理后重启不会复活旧 Run。
- **Workflow restore cap 顺序**：当前恢复数量上限先于全部有效性与 tombstone 判定。未来应先解析权威身份和清理事实，再对有效候选施加稳定上限，避免坏记录挤占额度。
- **Workflow sidecar repair**：seed fallback 目前只恢复内存投影，不把重建结果当作普通 CAS 写回。未来应提供独立 repair transaction，验证崩溃幂等性，且不能放宽正常 revision CAS。
- **Behavior projection freshness**：projection 与服务端 admission 已共用纯判定，但 foreground 变化后的 UI 广播仍可能短暂显示上一次快照。未来应在 admission facts 变化时按 revision 推送，并证明旧投影不能覆盖新状态。
- **Pager 图片草稿所有权**：v2.1.0 明确拒绝持久化临时图片路径。未来若需要图片恢复，必须设计独立 blob 生命周期、配额、加密/清理和引用完整性；验收要求崩溃后不泄漏路径、不悬挂 blob、不自动发送。
