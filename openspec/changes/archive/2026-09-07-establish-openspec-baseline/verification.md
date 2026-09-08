# 迁移验证

- OpenSpec 1.11.0：npm registry 确认版本可用，要求 Node >=20.19.0，CI 使用 Node 22。
- `openspec validate establish-openspec-baseline --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`（归档前）：1 个 change 通过。
- 41 项要求的源路径和符号逐项存在；Cargo 57 个成员均进入模块映射。
- 已阅读实现与现有测试作为事实证据；本次仅修改文档和 CI，没有运行 Rust 测试、Provider 网络集成或 OS 隔离回归。
- 共享工作树有其他进行中的代码修改；本次不修改这些文件，evidence.json 记录证据快照，后续集成需要重核。

## 归档后结果

- 官方 archive 成功创建 14 份主规范、合入 41 项 ADDED 要求。
- 首次主规范严格校验发现中文 Purpose 不足 50 字；已补充各能力的具体范围，重新校验通过。
- `openspec validate --all --strict --no-interactive`：15 项通过、0 项失败（14 份基线规范，另一个并发任务创建的 audit-goal-settlement-retry change 也通过格式校验）。该并发 change 的实现和任务完成不属于此次迁移。
- `openspec validate --archived --no-interactive`：本次归档通过，所有迁移 tasks 完成。
- 14 份主规范 Requirement 正文与对应归档 delta 一致；Purpose 已替换 CLI 自动生成的 TBD。
- 新建入口和 OpenSpec 文档中的 72 个本地 Markdown 链接有效；legacy 原文快照保持原始相对引用语义，不纳入新文档链接校验。
- README 与 ROADMAP 的 `git diff --check` 通过；新增文件另行检查行尾空白。
- 归档提示超过 10 个 delta 建议拆分。本次是一次性现状基线，因此保留同一个迁移 change；后续功能改动按受影响能力缩小范围。
- GitHub Actions 未在远端运行；已在本机执行 CI 中的两条 OpenSpec 校验命令。
