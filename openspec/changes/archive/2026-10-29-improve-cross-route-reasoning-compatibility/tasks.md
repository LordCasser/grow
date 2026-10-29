## 1. 审查与实现
- [x] 核对现有契约、调用方和对抗边界，记录本次范围。
- [x] HTTP/SSE 完整错误证据产生 typed reasoning 要求并保留 DTO 往返。
- [x] Responses 普通 assistant reasoning 投影与边界回归。
- [x] 更新开发说明，独立审查最终 diff。

## 2. 验证与归档
- [x] 执行相关 Rust 回归、格式与差异检查。
- [x] 构建本次 Grow 并完成真实跨 provider/model/API 切换，保存脱敏验证记录。
- [x] 核对工具执行次数、恢复预算、路由/native 隔离与真实失败限制。
- [x] OpenSpec strict 验证、归档、全量及 archive 验证；清理临时凭据与构建产物。
