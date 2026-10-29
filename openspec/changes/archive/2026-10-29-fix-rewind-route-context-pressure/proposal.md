## Why

rewind 恢复压缩前的历史时，ChatState 仍将原始 Surface 的增量加到旧 provider 上下文锚点；保留但不回放的 reasoning 也被计入。随后切模型只重置 continuation，未重建计数，可能显示远超模型窗口的上下文。冷恢复已有 portable replay 初始化规则，但 resident 分支和路由切换未覆盖。

## What Changes

- rewind 持久提交后、真实采样路由替换后，按新 continuation 的 portable replay 重建上下文压力和两项请求估值基线，立即发布展示事件。
- 复用冷恢复的估算路径，保留真实 portable overflow；后续请求继续加入实际 envelope，provider 回报继续校准。
- 不改变累计消费、Timeline 历史、普通配置更新及压缩的 provider overhead 增量规则。

## Capabilities

### Modified Capabilities

- `model-sampling`：resident rewind 与采样路由替换的上下文压力边界。

## Impact

ChatState state 初始化、rewind 和 ReplaceSamplingRoute；新增跨压缩 rewind、模型切换、重复请求及真实超窗回归。更新开发指南中的计数说明。
