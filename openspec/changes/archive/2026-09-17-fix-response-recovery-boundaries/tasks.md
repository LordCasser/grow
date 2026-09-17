## 1. Regression and implementation

- [x] 1.1 重复 rewind / compaction / cold fold 回归先失败后通过，保留原 response provenance。
- [x] 1.2 核对审计中的 fork 失败复现，并以真实 storage 回归验证父 projection 的继承历史转换。
- [x] 1.3 resident 初次 snapshot / delta 窗口回归先失败后通过，保留正确物理截点。
- [x] 1.4 exact projection/ACP file-sync 与 directory-sync 故障回归先失败后通过，重试保持去重。

## 2. Verification and documentation

- [x] 2.1 更新开发说明、执行受影响回归与必要 package check，记录已验证范围和限制。
- [x] 2.2 检查改动、changed-file rustfmt、diff 与 OpenSpec 严格校验。
- [x] 2.3 逐项核对场景后归档本 change，再验证主规范与 archive；保留其他进行中 change。
