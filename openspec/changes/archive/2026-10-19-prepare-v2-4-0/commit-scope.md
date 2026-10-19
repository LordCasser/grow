# 2.4.0 提交范围核对

本次按用户要求整理实现、文档、版本元数据和 OpenSpec 归档后提交到 Git；起始 HEAD 为 `8cc20454`，分支为 `codex/v2-4-0`。版本统一为 2.4.0，完整用户行为见 [changelog](../../../../crates/codegen/shell/changelogs/2.4.0.md)。

## 纳入范围

- 会话树快速挂起与同身份恢复、任务规划与父子弱关联、延期计划审阅、统一 Skill 加载四项功能及其已验证修复。
- 本版 backlog 代码核查所确认的缺陷，冷恢复事件 ID 补种，以及既有中文 text-only 拒绝和可空 Chat usage 明细修复。
- MCP catalog 刷新与生命周期、会话 MCP 身份和辅助策略刷新，以及问询持久身份前置。这些路径被本版 child 策略刷新、来源询问启动去重和恢复直接依赖，随实现与归档一起纳入。
- 相关主规范、开发者说明、归档后相对引用、验证记录和 backlog 收敛。MCP 多轮继续保持门禁关闭，`add-mcp-multi-round-elicitation` 保留为唯一 active change。

## 留在工作区的并行改动

以下四项独立 change 及对应代码、测试、文档和主规范增量不纳入本次提交：

- `2026-10-18-complete-image-fallback-lifecycle-regressions`
- `2026-10-18-harden-image-fallback-routing`
- `2026-10-18-verify-current-model-tool-image-fallback`
- `2026-10-18-fix-blank-ask-session-inquiry-id`

11 个共享文件通过 index blob 单独暂存，保留本版必要变更；其余排除项没有暂存。暂存前后的原工作区文件 SHA256 已逐项核对，拆分暂存没有改写源文件；之后仅修正下述本版测试夹具并新增本记录，其余原文件的 SHA256 保持一致。范围清单及候选内容保存在 `/tmp/grow-v2-4-0-scoped-projection/`，独立导出的暂存区快照为 `/tmp/grow-v2-4-0-staged129/`。只读复核确认四项排除 change 的独有行为没有混入暂存投影。

## 提交快照验证

此前 [verification.md](verification.md) 的全量回归是在保留并行改动的完整工作区运行。为验证实际提交内容，额外从 Git index 导出独立目录，并固定 Rust/Rustdoc 1.93.1、两编译任务、16 MiB 测试栈、禁用 incremental 与 dev/test debug 进行核对：

- `cargo check --locked -p cli` 通过，日志 `/tmp/grow-v2-4-0-staged129-check.log`。Pager 报告两项方法未使用的 warning，无编译错误。
- `cargo fmt --all -- --check` 通过，日志 `/tmp/grow-v2-4-0-staged129-fmt.log`。
- `openspec validate --all --strict --no-interactive` 为 16 passed / 0 failed，日志 `/tmp/grow-v2-4-0-staged129-spec.log`。
- `openspec validate --archived --no-interactive` 为 601 passed / 0 failed，日志 `/tmp/grow-v2-4-0-staged129-archive.log`。比完整工作区少四项，正是本次排除的并行 change。
- 初次 Shell 全量运行在其余测试完成后停于 `queued_and_direct_skill_steers_keep_images_literal_text_and_atomic_identity`；定向重现返回 stale steer。夹具把阻塞点设为 `[DONE]`，实际 `response.completed` 已可被消费，形成同步竞态。仅在该测试移除脚本末尾 `[DONE]`，使既有阻塞器停在真实终止事件前，并为 queued ACK 加 5 秒失败期限；不改生产行为或放宽断言。原日志保留为 `/tmp/grow-v2-4-0-staged129-shell-tests.log` 与 `/tmp/grow-v2-4-0-staged129-skill-focus.log`，不将中止运行计为完整通过。
- 修正后的 Skill 定向用例连续三次通过，日志 `/tmp/grow-v2-4-0-staged130-skill-focus.log`；重编和格式复核通过，日志 `/tmp/grow-v2-4-0-staged130-shell-build.log` 与 `/tmp/grow-v2-4-0-staged130-fmt.log`。
- 修正后的独立快照 Shell 全量回归为 **4239 passed / 0 failed / 5 ignored**，123.88 秒，日志 `/tmp/grow-v2-4-0-staged130-shell-tests.log`。本次只重跑共享文件拆分和夹具修正所影响的 Shell；其他包及真实进程的既有证据仍见 `verification.md`。
- `git diff --cached --check` 通过。

开发文档中关于 `bound-macos-clipboard-transfer-file` 和 `bound-terminal-image-escape-output` 的两条失效链接在起始 HEAD 已存在，本次保持原状；本轮归档迁移涉及的相对引用均可解析。

此次只创建本地提交，不创建 tag、不 push、不执行发布。
