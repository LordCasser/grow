## Why

用户授权实施当前全部打开的 change，并作为一次新版本更新交付。当前 workspace 为 2.3.1；本次包含明确的行为变化，目标版本为 2.4.0。需要统一实施依赖和发布证据，避免多个 change 对相同 owner、Timeline、输入与恢复各自增加互相冲突的状态。

## What Changes

本 change 只管理版本号、发布说明和跨 change 的集成验证，不增加产品契约，因此 `skip_specs: true`。具体行为由以下五个 change 的 delta 定义：

- `suspend-session-tree`
- `evolve-task-planning`
- `retain-plan-for-later-review`
- `unify-skill-loading`
- `recognize-text-only-content-type-rejection`

开始实施时工作区已有 `fix-null-chat-usage-details` 的已归档实现，保留其代码及证据并纳入采样回归，不重复建立其行为契约。实施启动后新出现的其他 change 不自动扩入本次发布。

## Capabilities

无新增或修改 capability；本记录不代替各功能 change。

## Impact

版本元数据、release notes、开发者说明、跨模块集成验证及已完成 change 的归档顺序。发布构建和验证遵循既有流水线；本次不自动推送 tag 或发布远程 release。

## 2026-10-18 scope decision

用户明确本版暂缓 MCP 交互，保留现有 SDK 门禁并完成其余 change。`add-mcp-multi-round-elicitation` 保持 active，不作为 2.4.0 已实现能力；当前没有可用的真实 Form/URL 服务，不声称互操作验证通过。

## 2026-10-19 backlog scope extension

用户进一步授权对 `openspec/backlog.md` 的问题逐项结合代码分析、确认、立项和修复。确认仍存在的行为缺陷分别建立或复用最小 change，再实现和验证；不把 backlog 文字当作现状证据，也不重复建立已由并行 change 覆盖的契约。新增产品方向和需要真实 provider 负载的优化假设单独列出，等待明确范围，不伪造验证结论。逐项事实与责任见 [backlog-audit.md](backlog-audit.md)。MCP 多轮交互的暂缓决定保持有效。

用户本轮再次确认“先修已确认缺陷，方向类保持暂缓”：Remote Control / Agent Host、Provider 缓存及 Recap 共享收益不在本轮启动。

联合验收另确认 headless noReplay 冷恢复没有从完整物理历史恢复 event ID 计数，阻断后续采样持久化；独立最小修复见 [reseed-cold-session-event-identities](../2026-10-19-reseed-cold-session-event-identities/proposal.md)，不放宽 exact append 冲突校验。
