## 1. 复现与修复
- [x] 1.1 核对 lease、挂起、前台 handoff 生命周期，并用真实 handle_prompt 复现互等。
- [x] 1.2 最小修复并验证 ACK 未确认时不提前 abort、deadline 后同身份可恢复挂起。
- [x] 1.3 对抗性审查、相关测试、开发说明、OpenSpec 严格校验与归档。
