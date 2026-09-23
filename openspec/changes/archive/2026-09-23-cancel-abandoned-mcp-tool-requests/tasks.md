## 1. Baseline

- [x] 1.1 复核 rmcp 2.2 的 request handle、无内建期限选项与响应类型；核对发起前/取得 id 后的同一预算、主/重试/direct 三条调用链、服务替换和 ACP bridge 限制。

## 2. Implementation

- [x] 2.1 在 MCP 层实现一次性 cancel-aware tools/call helper，id/peer Drop guard 和 settled/timeout 解除逻辑；无 runtime/发送失败不阻塞 Drop。
- [x] 2.2 主调用、恢复后的唯一重试及 direct 入口共用 helper；保留各自预算、业务错误、HTTP conditional reset 和不重放超时语义。

## 3. Verification

- [x] 3.1 用真实 rmcp duplex 对端按 id 测正常、超时、future drop、竞态、重试、服务替换和通知失败；测试 direct 调用预算。
- [x] 3.2 更新开发者说明并在 `verification.md` 记录命令、退出码、实际通知次数及 ACP bridge 限制。
- [x] 3.3 逐项核对 delta、执行全量 strict OpenSpec、归档本 change，再执行全量与 archived 校验；未验证项不勾选。
