# KV cache 优化的 change 拆分

本轮交付四份方案。运行时改动、开发工具和 provider 实验分别验收，当前尚未实施或归档。

| Change | 交付边界 | 前置 | 验收重点 |
| --- | --- | --- | --- |
| [stabilize-image-budget-projection](proposal.md) | 跨请求保留图片预算选择 | 无 | 首次回收后的追加不会每轮推进淘汰边界；native 安全边界保持有效 |
| [preserve-cache-usage-availability](../preserve-cache-usage-availability/proposal.md) | 缓存字段可用性及整条结算/展示链 | 无 | 缺失与 0 不混淆；known total 不误停预算；恢复、子 Agent、Sideband 同义 |
| [add-offline-prompt-cache-diagnostics](../add-offline-prompt-cache-diagnostics/proposal.md) | 读取现有证据的开发者离线工具 | 读取/比较可先做；最终用量核对依赖第 2 项 | 能归属本地变化与未知条件，不推断远端 miss 因果 |
| [evaluate-provider-cache-affinity](../evaluate-provider-cache-affinity/proposal.md) | 有界实验与按 route 的决策 | 前三项完成 | 对等工作负载的真实用量、延迟、错误与成本证据 |

## 实施顺序

1. 第 1、2 项没有行为依赖，可以分别推进；第 3 项的证据读取与请求比较也可先做。
2. 第 2 项完成后，第 3 项核对最终 usage 格式和覆盖口径。
3. 前三项验证完成并归档后开始第 4 项。候选只有获得 route 级证据才另开最小生产行为 change。

第 1、2 项各自有 delta specs。第 3、4 项为开发工具与实验，设置 `skip_specs: true`。各 change 自己负责 docs、验证记录和归档，不等待一个总开关发布。若顺序归档导致主规范变化，下一项需重新核对其完整 MODIFIED requirement 与仍成立的场景。

## 本轮已确认的设计边界

- 图片预算选择由 ChatStateActor 的请求投影状态持有，生命周期独立于 native reset；冷恢复、fork 等新域重新建立选择。
- full input/output 与缓存明细完整性分开。已知样本命中率必须带覆盖范围；normal 状态栏覆盖不完整时显示 N/A。
- 离线工具只读既有证据，使用显式请求对，数组顺序参与比较，不增加生产日志屏障。
- 暂不定义通用 provider cache registry、全局 TTL 或跨会话 key 共享策略。相关待验证事项已登记到 [backlog](../../backlog.md)。

## 方案验证

- `openspec validate --all --strict --no-interactive`：18 项通过、0 失败（14 个主规范 + 4 个活动 change）。
- 核对四项 proposal/design/tasks/metadata 均存在，文档相对链接可解析，MODIFIED requirement 名称能对应主规范且保留原场景标题。
- 契约与任务复核后，进一步明确 Goal cache-miss 只在单个 read-known attempt 内推导，并将 provider 实验矩阵逐格核验写入任务。
- `git diff --check` 通过；所有实施 tasks 保持未勾选。
- 本轮只编辑 OpenSpec 文档。没有执行 Rust 编译、运行时测试或真实 provider 实验；上述格式校验不能证明功能已实现或缓存收益。
