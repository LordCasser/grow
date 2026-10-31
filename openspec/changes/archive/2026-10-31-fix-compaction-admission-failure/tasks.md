## 1. 复现与修复

- [x] 1.1 完成内部准入拒绝与取消来源审计，记录生产写入来源和终态处理证据。
- [x] 1.2 先运行压缩期间存储域关闭的失败回归，再保留 storage Err；验证 Completed 保持、Turn 为 error 且无后继 provider 请求，并验证另一 session 单独失败不影响当前会话。
- [x] 1.3 以受控 ACK 检查异步取消在发布前及提交后的竞态，验证 generation 与 foreground 的取消作用域。
- [x] 1.4 执行相关 Shell 回归、定向格式/diff 检查，更新开发者说明与验证记录，通过 OpenSpec 全量严格校验后归档并校验 archives。
