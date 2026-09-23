# Verification: prune-resolved-backlog

## Scope

本次只清理 `openspec/backlog.md`，不修改主规范、源码、测试或其他 active change。核对依据是当前工作树；`backlog.md` 和若干近期 archive 在本次开始前已经是并行 dirty/untracked 状态，未被本 change 重写。

## Removed records

以下整段只有已完成事实、审计限制和归档指针，没有遗留待办，因此从 backlog 删除，详情由对应 archive 保留：

- `Recap 与压缩摘要的工具证据保留`：`2026-09-17-fix-compaction-budget-recovery` 及其审计归档。
- `无签名 Messages 回退切断工具往返`：`2026-09-10-preserve-portable-tool-exchanges`，保留的 audit scope/限制不构成未完成行为。
- `Pager 恢复回归夹具与首条输入队列`：`2026-09-22-repair-pager-recovery-regressions`，验证记录为 7,242 passed / 0 failed / 11 ignored。

以下独立单行同样只保留归档已完成事实，因此删除：

- 第三轮恢复边界与依赖：`2026-09-13-remove-runtime-dependencies-from-leaf-crates`。
- 更新目录、LSP inspect 两项、web_fetch 两项、MCP 六项、Hook 三项：对应 `2026-09-07-*` archive verification。
- 技能展开诊断、剪贴板统计、复制文件原子提交、Minimal transcript reload、Minimal FPS HUD、词选择提示测试误报、TUI doctor：对应 `2026-09-07-*` / `2026-09-08-background-doctor-report` archive。
- 同进程 writer lease 退出边界：`2026-09-08-join-session-persistence-owner`。
- 会话读取 Summary 投影：`2026-09-13-keep-session-observation-read-only`。
- query 内图片 normalize notice：`2026-09-22-surface-inline-image-normalization-notices`。

所有被删除记录中的显式 `changes/archive/...` 链接均在当前工作树解析到实际文件；未显式链接的 change 名称也核对到对应 archive 目录。

## Kept mixed records

混合段没有整段删除，原因如下：

- 子 Agent 审核与 portable 工具历史的标题已同步改成“仍待处理/仍待定义”，避免完成标签覆盖同段剩余债务。
- 子 Agent 审核仍保留 typed diagnostic reason 与跨 child/外部写者冲突保护。
- 恢复审查仍保留真实长会话输入 p95、Workflow checkpoint 和在线 Timeline 事项。
- Responses phase portable、结构化 portable tail、相同原始 tool ID 的 neutral/native 歧义仍未完成。
- Hook/skill、分页器响应性、剪贴板/图片资源预算、配置 rollback/CAS、日志配额、draft 删除持久化、rewind 结果归属和 CLI trace 覆盖均仍有明确后续边界。
- `HTTP Hook 请求边界`、`分页器失败反馈`、`分页器反馈来源`、`TTL 清理候选句柄` 等段落虽然包含完成的局部修复，但同段保留未审计范围、Minimal 子 Agent 一致性、IO 错误策略或扫描失败覆盖，不删除。

## Dirty-tree caveat

`git status` 在本次核对前已显示 `openspec/backlog.md` 修改，以及 2026-09-17 之后多个 archive 目录未跟踪。清理 change 不创建这些 archive，也不把它们加入其他 change；提交本 change 前必须确认被删除记录所指向的 archive 会与其所属变更一并保留。

`openspec/changes/verify-openspec-baseline` 是 `2026-09-07-verify-openspec-baseline` 的旧 active 副本：archive 已包含对应 proposal/design/tasks、evidence 与完成记录，active 目录没有独有规范或实现内容；active 的旧未勾选任务不代表新的开放债务。root 另行删除该重复 active 目录，本 change 的 backlog 清理和归档不依赖该删除。

`bound-sips-output-reads` 的一次测试 EPERM 在后续 2,000 次实际 runner 退出中未复现，生产阶段错误已有处理。backlog 只保留再次出现时记录带 stage 标签错误的证据触发器，并移除过时的 `investigate-sips-process-eperm` 立项指针。

## Checks

- Archive link existence: pass for every explicit `changes/archive/...` link remaining in the current backlog and all removed records.
- `git diff --check`: pass.
- `openspec validate --all --strict --no-interactive`: pass before root's separate deletion of the duplicate active `verify-openspec-baseline`; a later rerun in the shared dirty tree reports that expected deletion as an unrelated missing-change error.
- No Cargo/test/build command was run.
