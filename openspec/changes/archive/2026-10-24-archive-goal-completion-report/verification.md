# 验证记录

日期：2026-10-24。

## 自动验证

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p tools -p shell -p pager --lib goal -- --test-threads=2`：245 项通过（Pager 57、Shell 183、Tools 5），无失败或忽略项。
- 修正 Workflow 历史点击分支后，`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p pager --lib goal -- --test-threads=2` 再次通过 57 项，包含历史 Workflow 与 Done Goal 并存的真实鼠标 handler 回归。
- 验证包含：缺失/空白报告拒绝、blocked 不能附报告、过期 authority 拒绝、必需交付门槛、完成状态与报告的 Control 确认、写入失败回滚且不发布成功、GoalView 原文、快照恢复、迟到用量保全、编辑清除、typed history 往返、捕获 Control 补足遗漏的完成投影、Pager live/replay hydration、Markdown 展示和长报告末尾滚动。
- 初次编译暴露三个旧 fixture 的无参数 `complete()` 调用；适配报告参数后重新编译并通过上述测试。
- 定点 `rustfmt --check` 与 `git diff --check` 通过。原有弃用 API 及 macOS linker unwind 警告不属于本次变更。
- 归档前严格全量 OpenSpec 验证通过 17 项；归档后严格全量通过 16 项，archive 校验通过 707 项。变更已合入 `behavior-goal` 和 `client-surfaces` 主规范。

## 对抗性复核

沿工具输入、GoalTracker、Control、GoalView、GoalUpdated、GrowHistoryFact、实时/回放投影到详情查看核对字段归属及失败路径；原有完成审计门槛未被报告输入取代。缺失历史报告仅展示未归档，不补造完成原因。

发现既有鼠标分支优先查看任何 Workflow 历史，会截走可见 Goal 的点击；本次改为当前 Goal 优先，定向回归补入已完成 Workflow 历史。

## 边界

使用生产 handler、状态提交路径、typed history 与 Ratatui buffer 验证，没有真实 provider 调用、发布二进制或交互式终端人工验收。报告是否足以证明业务完成仍由模型的整体完成审计负责；runtime 只验证报告非空及原有完成门槛。

工作区已有其他未提交改动，本次保留它们；验证针对当前组合工作树。确认无运行中的 Cargo/rustc 后，清理本轮修改的 shell、pager、tools 增量缓存 12.91 GiB，保留已链接产物。
