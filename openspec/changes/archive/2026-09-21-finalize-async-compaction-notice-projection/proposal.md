## Why

异步压缩在 Surface replacement 提交后立即发布 `AutoCompactCompleted`。此时 ChatState 只把 Surface token 差量应用到旧 provider anchor，旧请求的 tools、system reminder 等 envelope 仍在投影中；下一次普通请求完成组装后，`apply_request_projection` 会用真实 request 重新计算，状态栏因而可能很快从通知中的约 308k 变为约 189k。两个数字描述不同阶段，却都被 UI 表述为压缩后的最终上下文。

## What Changes

- 异步压缩提交后暂存完成通知所需的压缩前 token 数和耗时，不再发布 Surface-only 中间值。
- 下一次普通模型请求完成 request projection 后，读取同一 ChatState 投影并发布一次 `AutoCompactCompleted`。
- 没有后继请求时保持通知待发布且不显示中间值；待发布期间不启动另一轮后台压缩。
- 同步提升、同步自动压缩和手动压缩继续使用各自现有的即时完成通知语义。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `context-compaction`：异步完成通知的发布边界和 `tokens_after` 含义。

## Impact

修改 Shell 的 compaction runtime state、异步发布路径、普通请求组装后的单一通知出口及现有异步压缩集成测试。通知 schema、Pager 渲染、Timeline 压缩事务和请求 token 估算算法均不改变。
