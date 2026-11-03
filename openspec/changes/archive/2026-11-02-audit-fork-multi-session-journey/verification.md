# 验证记录

## 范围与版本

- 核对 OpenSpec 索引、session-timeline 的删除/挂起/分支契约、client-surfaces 的加载/退出契约、活动 MRTR proposal 和既有 fork/close/delete 测试。
- Atlas 发现包含已移动文件的历史索引条目，所有结论以当前源码回读为准，不把空调用查询当成全仓证明。
- 安装版 `grow 2.5.5 (5998735)`；工作区 HEAD `bbd2c4dfa`。`git diff 5998735 --` 核心受审删除、Dashboard、close、fork failure 与 quit gate 文件无差异；临时测试与并行恢复工作另行记录。

## 执行结果

| 验证 | 结果 |
| --- | --- |
| Pager session dispatch 既有测试 | 212 passed |
| Pager Dashboard dispatch 既有测试（排除审计探针） | 154 passed |
| Pager quit 既有测试 | 36 passed |
| Shell 删除护栏、writer teardown、close、生命周期串行化 | 6 passed |
| 审计 Pager 期望断言 | 6 failed，分别复现 A4、A5 两种状态、A6 两种症状、A7 |
| 审计 storage 期望断言 | 1 failed，复现 A1 |
| 安装版对 Gall's Law 完整临时副本执行 CLI delete | exit 1，sampling evidence bounded budget，原副本保留 |
| 安装版对 Count external library 完整临时副本执行 CLI delete | exit 1，unfinished runtime handoff，原副本保留 |

408 项既有定向回归通过。7 个审计失败是为了暴露当前行为而编写的期望断言，不是修复后的测试结果；本 change 没有产品修复。源码与原始有界输出分别见 `pager-probes.rs`、`storage-probes.rs`、`probe-results.txt`、`cli-probe-results.json`。

构建入口为 `cargo test --locked -p shell -p pager --lib audit_fork_journey_ -- --test-threads=1 --nocapture`。Cargo 在 Pager 失败后不再执行 Shell，因此另行直接运行同次已生成的 Shell test binary。最后一次 Pager 探针使用 `cargo test --locked -p pager --lib audit_fork_journey_ -- --test-threads=1 --nocapture`，6 项均在目标断言失败。

既有回归使用同次构建的 test binary 串行执行以下 filters，避免重复编译与 feature 集合变化：

```text
Pager: app::root::dispatch::tests::session::
Pager: app::root::dispatch::tests::dashboard:: --skip audit_fork_journey_
Pager: quit --skip audit_fork_journey_
Shell: explicit_delete_preserves_unfinished_runtime_handoff_but_allows_consumed_audit
Shell: session_delete_tears_down_resident_actor_before_releasing_lifecycle
Shell: explicit_close_
Shell: session_lifecycle_gate_serializes_close_behind_load_incarnation
Shell: delete_session_removes_dir_and_is_idempotent
```

复现探针时，应在隔离 checkout 内把 `pager-probes.rs` 临时附加到 `pager/src/app/root/dispatch/tests/dashboard.rs`，把 `storage-probes.rs` 临时附加到 `shell/src/session/storage/jsonl/tests.rs`，再用上述 filter 执行。它们复用原测试模块 fixture，不是可独立编译的 Rust 程序。运行后撤回临时附加内容。已归档的 probes 不参与默认测试编译。

本次已精确移除临时附加的两个 block，两个生产测试文件的 diff 均为空。没有 git reset/checkout 覆盖用户或其他工作的改动。

## 本机副本与限制

- 只读提取截图对应 summary 的身份/来源字段与 Timeline 事件类型，不在审计中复制对话正文、模型输入或凭据。
- CLI 副本放在 `TemporaryDirectory` 下，子进程显式设置 `GROW_HOME`；没有连接真实 leader，也没有向真实 actor 发请求。删除失败后副本仍存在，验证前后原 Timeline hash 相同；随后临时目录自动回收。
- 会话仍可能被用户或另一项授权工作推进，文中的规模计数是一次只读快照，不声称是截图时刻或永久冻结数据。
- A3 为源码确认的跨层失败顺序，A8 还由既有测试明确保留 placeholder 的断言支持；未执行真实 resident 破坏性删除或 fork 网络/磁盘故障注入。
- A4/A5/A6/A7 为隔离 reducer/dispatch 复现，不声称已完成真实键盘 → ACP → actor → 磁盘的 TUI 端到端验证。
- G1 是现有保护与产品缺口；R1 未跑双客户端退出，不作为已复现缺陷。未运行真实 Git worktree fork、模型请求或整个 workspace 测试。
- 构建有既有 Atomic::fetch_update 弃用警告和 macOS unwind-table 链接警告。中间一次重复构建被并行 `fix-suspended-project-rules-resume` 的临时诊断格式编译错误打断；该任务自行处理后，本次最终 Pager 探针构建完成。未修改其代码。

## OpenSpec 与收尾

归档前 `openspec validate --all --strict --no-interactive`：17 passed，0 failed。

临时探针只存在于测试二进制和本 change 证据中；已删除本次三个测试可执行文件及两个 Pager 增量缓存，回收约 5 GiB。Shell 增量缓存被并行恢复工作复用，未清理共享 library/该任务缓存。

归档为 `2026-11-02-audit-fork-multi-session-journey`，未修改主规范（skip_specs）。归档时最后一项收尾校验尚未勾选，首次 archived 校验仅因此报告 1 incomplete task；完成收尾后勾选并重跑。

最终 `openspec validate --all --strict --no-interactive`：16 passed，0 failed；`openspec validate --archived --strict --no-interactive`：765 passed，0 failed。`git diff --check` 通过；生产测试文件无本次 diff，临时 probe 标记已全部移除。Backlog 已更新为归档后的证据链接。
