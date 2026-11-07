# Tasks: reject-repeated-journal-completions

验证方法（实现前约定）：Tier 2。新回归必须经真实公共入口——`Journal::load_storage`（注入 `FileJournalStorage`）、真实临时文件 `Journal::load`、`Journal::memory` + `project_physical_entry`——断言同一 seq 的第二次 completion 行（同值/异值）被 `JournalError::Sequence` 拒绝且已 fold 结果不变、文件不改写；live `complete_operation` 对照断言 Pending→Completed→same/different 均为 Divergence 且合法一次完成持久为两行。旧代码上新 restore/projector 回归必须失败（覆盖发生）、live 对照在旧代码上已通过。验证命令：`cargo test -p workflow --lib --locked --offline journal::`（父在 WS02 窗口执行，本 worktree 不跑 cargo）；`rustfmt --edition 2024 --check` 自查触碰文件；`openspec validate reject-repeated-journal-completions --strict --no-interactive`。

- [x] 1. 建 proposal、delta（WHEN/THEN）、design、tasks；`openspec validate reject-repeated-journal-completions --strict --no-interactive` 通过。
- [x] 2. 实现 `fold_physical_entry` 严格一次 completion 守卫：`existing.result` 仍携带与 `operation_ids[index]` 相同的 pending marker 才允许替换；第二次 completion（同值/异值）落入既有 `JournalError::Sequence`，不改已 fold 结果、保留 operation id 历史、不动 live 与 prune/record 路径。
- [x] 3. 测试：restore 双入口（`load_storage` 同值行、临时文件 `load` 异值行）断言 Sequence 拒绝且文件不变；`projector_rejects_a_second_completion_and_keeps_the_first_result`（同值/异值 + 原结果不变）；`live_complete_operation_rejects_a_second_completion`（same/different 对照 + 合法完成两行持久）。测试已写入并自查，执行依赖第 5 项。
- [x] 4. 自文件 rustfmt 与最终 diff 自查（不跑 cargo、不 workspace fmt、不动 Shell/backlog/其他 change）。journal.rs SHA256：`50860f56d3cb586d434780ca2ef30cc2f26e4b3e72584e3fc207a4d9a43e0f2c`。
- [x] 5. 父在最终整合源码运行 `cargo +1.93.1 test --locked --offline -p workflow --lib -- --test-threads=1`：69/69通过，含4条新增公共入口回归；父核完整diff与调用方，不改变F4存储接口，开发说明同步。
