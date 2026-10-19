# 验证记录

- `rustfmt --edition 2024 crates/codegen/shell/src/agent/mvp_agent/runtime_handoff.rs`：通过。
- `git diff --check`：通过。
- 主 agent 集中编译生成了 shell lib 测试 executable。
- Ignored 探针 `agent::mvp_agent::runtime_handoff::tests::canonical_child_facts_owned_read_matches_borrowed_clone_path`：1 passed，0.61 s。样本为 8,002 个事件、34,095,721 序列化字节；旧 borrowed `to_vec` + replay 路径 10,068 µs，owned replay 路径 3,675 µs。该结果是一次本机微基准，fixture clone 在计时外；仅用于确认路径等价并观察被移除复制的局部代价，不外推为重复稳定耗时、端到端挂起性能或跨平台速度。
- `agent::mvp_agent::runtime_handoff::tests`：8 passed，0 failed，1 ignored；新增的 duplicate spawn、terminal/session mismatch 与损坏序号拒绝测试均通过。Ignored 测量测试随后单独运行并通过。完整事件验证仍由 `Timeline::from_events` 执行；冷 load 走 `load_light` 返回的 validated Timeline，再跑 CanonicalChildFacts 的关系检查。
- hosted 环境未验证。挂起 deadline 与 child retire 后 latest-facts reread 保持原位。

## OpenSpec 归档核对

2026-10-19 已通过 `openspec archive <change> --yes` 归档；全部 tasks 完成。归档前全量严格校验 23/23；归档后 `openspec validate --all --strict --no-interactive` 16/16，`openspec validate --archived --strict --no-interactive` 634/634。F2 先于 F3 归档，核对主规范同时保留 cursor merge 与 child lifecycle/实际历史顺序场景。开发说明和 backlog 已同步，无 MCP 实现修改。
