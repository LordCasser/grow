## 1. Exit and recovery

- [x] 1.1 核对 Quit→runtime→writer drain→恢复所有者，保留移除 viewer guard 的修复并说明不变量。
- [x] 1.2 添加普通/minimal 的后台完成通知回合双 Ctrl+D/冷恢复 PTY 场景，检查原身份、checkpoint、无取消和无重放。
- [x] 1.3 将新回归登记 core-regression，同步开发者说明。

## 2. Validation

- [x] 2.1 用仓库指定 toolchain 构建本次默认 CLI；对同次程序运行新 PTY 和既有递归树恢复回归，记录实际结果。
- [x] 2.2 复用前序 UI 对抗回归并以本轮真实按键/进程验证闭环，完成格式/diff、对抗性核对及 OpenSpec strict，归档并校验 archive，清理本次临时残留；额外 UI 补跑未执行情况如实记录。
