## Why
真实会话手动压缩显示0，后续投影假超窗，已提交压缩又缺少展示终态。审计还发现model-switch失败和suppression可绕过硬窗口准入。用户要求全面审计并修复这些相邻通路。

## What Changes
- 统一ChatState有效请求估值与同路由provider校准，消除raw差分提前饱和；覆盖append、replacement、prune、image、repair、rewind及route。
- 压缩后用有效投影报告结果；使hard overflow与自动压缩策略分离，传播model-switch失败。
- 配对压缩展示终态，新的replacement在提交时撤销旧pending async notice，Pager回合终止清理所属压缩反馈。
- 对真正仍超窗保留当前有界失败契约；不引入无界或新的多轮压缩框架。

## Capabilities
### Modified Capabilities
- model-sampling：有效上下文计数与硬窗口准入。
- context-compaction：投影完成值、失败与通知终态。
- client-surfaces：压缩反馈收尾。
