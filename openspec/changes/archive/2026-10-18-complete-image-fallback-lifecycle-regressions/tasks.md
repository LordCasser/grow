## Implementation and regression

- [x] 1.1 补充 task-local OCR 可执行文件替身与整链路成功/失败回归。
- [x] 1.2 覆盖 auxiliary 准入前/准备 ACK/运行中及 OCR 进程运行中取消，断言无后续 auxiliary/primary 请求及投影。
- [x] 1.3 从真实完成描述与 JSONL 冷恢复新 actor，覆盖投影前 crash window 和已投影历史复用。
- [x] 1.4 对失败回归确认根因，只修复本范围生产缺陷并执行对抗性边界检查。

## Verification

- [x] 2.1 运行新增回归及相邻图片/Sideband/存储检查，明确共享工作树隔离范围。
- [x] 2.2 完成 OpenSpec 严格验证和归档，清理本轮临时 Rust 产物。
