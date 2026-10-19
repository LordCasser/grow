## 1. Reproduction and implementation

- [x] 1.1 记录真实 PTY 前置失败与 checkpoint 诊断，核对既有契约。
- [x] 1.2 修复草稿交接并加入真实 worker/session binding 回归，不绕过持久化门槛；Pager 全量中的对应 worker 测试通过，见 verification。

## 2. Verification

- [x] 2.1 执行相关草稿单元测试、普通历史恢复与 root/嵌套挂起 PTY，记录实际结果；Pager31 全量及 CLI73/harness76 的 PTY77 七项运行通过。
- [x] 2.2 同步开发者说明，执行格式和 OpenSpec strict 校验；Pager fmt83、change strict83 与全量 strict83（24/24）通过，核对场景后归档。
