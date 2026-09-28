## Why

native continuation 合法性与 provider 缓存分组采用了相关但不同的边界。现有 key 含 timeline、rewind、route 与 epoch，但尚无真实 provider 数据证明哪类重置值得调整；同一协议、同一 provider 的不同模型和部署路径也不能预设具有相同缓存语义。

## What Changes

- 核对三个已归档前置 change、Grow 的 key/breakpoint/recap 实现，以及 OpenAI、Claude、DeepSeek 当前官方缓存契约；route 矩阵保留模型与部署身份未知的边界。
- 用合成 fixture 和离线 mock 验证证据格式与预算停止；对 key 生命周期、显式 breakpoint、retention 作理论推导和代码 review，不发送真实 provider 请求。
- 给每项候选写保留现状、另开行为 change 或证据不足的结论；修正 recap 的不实缓存注释，登记仍需真实 route 验证的债务。

## Capabilities

无产品契约变更。`.openspec.yaml` 设置 `skip_specs: true`：本项交付理论论证、代码 review、离线 mock 和决策，不发布生产参数、配置字段或新的路由行为，不虚构未来策略的 delta。

## Impact

mock 脚本、合成 fixtures、route 矩阵和报告均放在本 change 内；通用离线读取能力复用前置工具。修改 `openspec/backlog.md` 登记后续边界。本次验收不要求真实请求或跨 provider 实测结论，报告不声称缓存收益、命中率、延迟或费用改善。

前置：[图片投影](../2026-09-28-stabilize-image-budget-projection/proposal.md)、[缓存计量](../2026-09-28-preserve-cache-usage-availability/proposal.md)、[离线诊断](../2026-09-28-add-offline-prompt-cache-diagnostics/proposal.md)。生产优化需有具体 route 的可核验证据，另建最小行为 change，明确契约后再实现。
