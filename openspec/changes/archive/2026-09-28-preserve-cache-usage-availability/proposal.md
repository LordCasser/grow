## Why

Chat 与 Messages 的缓存字段缺失会在归一化时落到 0，Responses 的 cache write 当前固定为 0。多 provider 混合统计因此无法区分未命中与未报告，累计命中率也会被未知样本稀释。现有契约覆盖整个 attempt 的未知用量，没有定义缓存子字段的可用性。

## What Changes

- **BREAKING**：调整现有 normalized usage、账本和用量投影，使 cache read/write 分别保留缺失与显式 0；所有生产者、持久化和展示消费者同步更新。
- 按已验证的协议字段解析缓存读写；别名优先级依据字段是否存在，冲突保留诊断，不以数值为 0 触发 fallback。
- 缓存命中率只用 read 与 full input 均有效的样本，显示输入 token 覆盖率；normal 状态栏在覆盖不完整时显示 N/A，详情解释已知样本。
- 保留 full input + output 的总消费、逐 attempt 幂等结算、子 Agent 折叠和 Sideband 归属；区分总量不完整与仅缓存明细不完整。
- 确定总量的 Goal 预算继续按全量 token 工作，缓存明细缺失不单独关闭其准入。

## Capabilities

### Modified Capabilities

- `model-sampling`: 缓存字段可用性、协议归一化及全量输入的可信边界。
- `session-timeline`: 缓存明细及覆盖分母经过结算、折叠和恢复保持一致。
- `client-surfaces`: `/usage`、普通状态栏及结构化用量的可用性展示。
- `behavior-goal`: 明确总 token 预算与缓存明细完整性的关系。

## Impact

涉及 `sampling-types`、`sampler`、`chat-state`、Shell 主/子/Sideband/Goal 用量链路、ACP/headless 投影和 Pager。实现时更新 `docs/development.md` 的用量说明。不增加独立计费服务、价格表、provider registry 或历史账本重算流程。

无前置 change，与 [图片投影](../stabilize-image-budget-projection/proposal.md) 独立。[离线诊断](../add-offline-prompt-cache-diagnostics/proposal.md) 可并行开发读取部分，在本项完成后核对归一化计量；[provider 验证](../evaluate-provider-cache-affinity/proposal.md) 依赖本项。
