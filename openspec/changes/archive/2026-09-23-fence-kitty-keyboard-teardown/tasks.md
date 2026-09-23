## 1. Baseline and ownership

- [x] 1.1 复核 `event_loop`、正常 quit、连接失败、panic/signal、writer/TTY 所有权及相关测试，保存实施前基线和与并行改动的边界。
- [x] 1.2 将 reader 停止句柄交给恢复所有者，提供有界停止确认；reader 未停时不并发 drain/DA1。
- [x] 1.3 核实 writer `join`/Drop、stderr 写锁不会让新 fence 承诺的退出上界失效；仅修改必要的安全前置路径，失败仍可 best-effort 恢复。

## 2. Fence

- [x] 2.1 实现可单测的 DA1 解析与限时读写，明确 query 写失败、EOF、partial、非 DA1 的结果。
- [x] 2.2 按 reader 停止 → writer 收束 → teardown/pop → DA1 fence → raw restore 接入正常路径；无 flags/非 TTY/Windows 和异常路径保持对应快速语义。

## 3. Verification

- [x] 3.1 单测解析、条件决策、恢复顺序、reader/writer 失败和 panic/强制路径不等待。
- [x] 3.2 实际运行 PTY 的晚到 release、沉默 DA1、无 flags 三个场景，并回归普通退出与双 SIGINT；未运行须明确记录原因。
- [x] 3.3 更新相关开发者说明，以链接归档规范解释 fence 所有权；记录命令、退出码、覆盖场景和剩余限制到 `verification.md`。
- [x] 3.4 逐项核对 delta，完成后执行 `openspec validate --all --strict --no-interactive`，归档本 change，再执行全量和 archived 校验；未验证事项不勾选。
