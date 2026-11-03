# 验证

使用仓库 Rust 1.93.1（`/Users/lordcasser/.cargo/bin` 优先），复用共享 target，限制 jobs=4。验证不访问付费 provider。

## 回归先红后绿

- 新 `prompt::context::tests::observation_guidance_reaches_the_production_role_without_changing_the_head` 使用生产 `PromptContext::render()` 与 `render_role_with_renderer`。`scripts/run_exact_cargo_test.py --package agent --lib <完整名称>` 在修复前明确运行 1 项并失败，role 仅含 standard 文本，无观察提示。
- 最初直接 Cargo 使用未限定完整名称加 --exact 运行了 0 项；这次结果未计入通过，随后改用上述 exact runner 得到真实失败证据。
- 移动模板后 `cargo test --locked -p agent --lib -- --test-threads=4`：489 passed。补入真实 Builder 最终 role/head 的断言、修正 resume 文案的旧断言后，联合验证中的 Agent 全组仍 489 passed。
- Context 回归覆盖 primary/subagent × Full/Extend、默认/重命名工具、只有 status 和空工具集合；稳定 head 不变、动态 role 只引用可用能力。Builder 原 allowlist 回归实际构建 Agent 并检查最终 role 包含 list/status/wait，而 head 不含具名入口。
- `tool-types` 全组 127 passed；Task 的恢复说明保留 full transcript 和同类型约束，并明确重建 runtime/tools、不复制 prior permit。

## 边界

这些是模板渲染、工具名称装配和真实 Builder 的验证，不是模型遵循率评测。整个发布候选的 MCP、watcher 和 cold-load 失败见相邻发布准备 change；不将它们写成已通过，也不借此扩大本次 prompt 修复。

归档前后 strict / archived 结果由下方收尾记录补入。

归档成功，local-coordination 的既有 primary guidance Requirement 已更新。归档后全量 strict：Totals: 18 passed, 0 failed (18 items)；archived strict：Totals: 826 passed, 0 failed (826 items)。最终 cargo fmt 与 git diff --check 通过。
