## 1. Reproduce and fix

- [x] 1.1 增加可控 start/ACK/query 屏障回归，记录旧实现确定性失败及修复后通过。
- [x] 1.2 统一 runtime admission/query 边界并接入 backend；验证接纳失败、取消、相同身份去重与冷恢复不重跑。

## 2. Close the validation gap

- [x] 2.1 为相关 main push 启用协调 CI，更新开发说明；核对触发范围、YAML 与 diff。
- [x] 2.2 运行相关 Shell 回归及现有真实 CLI 协调进程脚本，记录准确命令、结果和覆盖边界。
- [x] 2.3 写明此次失败根因、清理日志误归因、此前 actor/runtime/进程/CI 覆盖缺口和修正措施。
- [x] 2.4 完成对抗性审查、OpenSpec 全量 strict 校验及归档校验，核对后归档。
