## 1. Regression and attribution

- [x] 1.1 添加 ACP notification → viewer → 双 Ctrl+D 的跨入口回归，核对 ACK 前无 Quit/Cancel、ACK 后退出。
- [x] 1.2 用旧 guard 使该测试失败，再撤回 guard 使其通过，保留准确命令和失败位置。
- [x] 1.3 对齐事故 notification Turn 与 shutdown prompt，补充根因及 UI 内存证据限制。

## 2. Closure

- [x] 2.1 检查最终 diff、Rust 格式和 OpenSpec strict；完成后归档并校验 archive，不修改并行工作。
