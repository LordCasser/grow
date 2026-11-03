## Why

recap 与 Memory Flush 会继续发送 Surface 中已有文字描述的原图，切换到纯文本模型后可能触发图片类型 400。前轮审计已定位两处遗漏，用户本轮授权修复。

## What Changes

- 在现有 Sideband 冻结边界等待主图片转换 durable ACK，并在请求副本优先选择已确认文字描述。
- recap 保留原有 reasoning 与预算规则；Memory Flush 保留压缩前 frozen source、revision、完整工具对和近期窗口预算。
- 按最终实际发送的模型路由检查尚未描述的图片；已知纯文本路由只读失败，未知路由保留原有首次乐观发送。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：recap 复用主会话已确认的图片文字分支。
- `memory-search`：flush 在保留冻结工具证据的同时复用已确认图片描述。

## Impact

修改 shell actor 的 shared Sideband freeze、recap、Memory Flush 和相应回归；同步开发说明及 backlog。无新增依赖、持久化格式或图片恢复 owner。辅助指定模型跨 provider 路由问题继续独立登记，本轮不扩展 resolver 语义。
