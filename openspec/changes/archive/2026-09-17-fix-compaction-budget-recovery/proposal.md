## Why
阈值附近的摘要请求只预算输入，却继承主采样输出上限；超长后的 Fitted/Simplified 会裁剪或删除工具证据而仍替换原 target。Size 失败及替换后仍超窗会永久抑制自动压缩，后续 prompt 的 overflow 恢复也被阻断。用户无法提供原 session，不能把源码复现等同于该次线上事件。

## What Changes
- 为压缩请求显式保留输出预算和输入余量，以完整摘要请求校验预算。
- 超预算/明确 provider overflow 时只在完整边界缩小选区，同步缩小摘要来源与 replacement target；保留调用参数、结果和附件，取消丢工具结果的降级。
- 输入大小失败只抑制当前 turn；成功提交后仍超窗仍有界结束当前 turn，但不封死后续恢复，通知提供重试路径。
- 修复 recap 同类工具尾部误删，复用 portable 投影。

## Capabilities
### Modified Capabilities
- `context-compaction`: 有界完整来源、摘要输出预算与可重试失败。
- `model-sampling`: recap 保留已完成工具证据。

## Impact
Shell 压缩准备、摘要请求、抑制状态与 recap；chat-state 范围预算辅助函数；不改持久化 schema、不修改用户账本、不新增模型路由/后台框架。
