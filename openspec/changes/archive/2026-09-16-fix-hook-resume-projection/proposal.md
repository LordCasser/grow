## Why

恢复会话时，Shell 在 ACP 对话重放后补发全部已完成 Hook，Pager 将其逐项追加为独立生命周期行，长会话因此出现上千行尾部噪声。投影丢失 Timeline 已有的工具身份，且补发复用实时路径会关闭 rewind 窗口；现行规范没有定义这些恢复展示边界，本次补齐契约。

## What Changes

- Hook 投影携带精确工具调用身份，并明确区分历史快照与实时执行。
- 恢复的工具 Hook 回附对应工具；保留合并 Edit 的调用归属，多次补发按 occurrence 去重。
- 无工具锚点的历史生命周期、隐藏工具及其说明收进一个可展开的历史 Hook 记录，保留成功、失败及阻止详情。
- 快照只读已完成 Timeline 事实，不执行 Hook、不写新的持久记录、不改变 rewind 状态。
- 实时工具 Hook 同样使用精确身份，避免并行工具错误挂接。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`: Hook 恢复展示、归属与幂等。
- `extension-runtime`: Hook 历史快照的只读边界。

## Impact

Shell HookExecution DTO、投影构造与补发，Pager tracker、Hook 分发及生命周期块渲染，相关回归与开发说明。Timeline 持久格式与事实权威保持不变；不重建两个日志之间不存在的全序，不扩展 Hook 执行策略。
