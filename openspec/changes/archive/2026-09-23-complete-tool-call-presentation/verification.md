## Evidence

`ToolInput` 是封闭枚举；开始投影此前唯一未列举的静态内置变体是 `Lsp`，动态变体虽显式列举却使用泛称。`ToolOutput` 的通配分支吞掉 `ContextRecall` 和 `Dynamic`，因此成功结果没有 ACP 终态。改动后两个 match 均无 wildcard，新增变体会在 Shell 编译时报出未覆盖。

完整列举不要求所有输出立即 Completed：现有 backgrounded Bash 结果刻意保留 InProgress，直到后台任务结束；delta 仅要求显式投影决策和已完成的成功结果关闭工具行。

生产转换回归直接调用 `SessionActor::send_tool_call_start`，检验 LSP 操作/文件与动态工具 wire name；结果转换回归检验 ContextRecall/动态输出的原 tool-call ID、Completed、正文和 raw output。Pager 通用成功工具行已将 content 作为 output 展示；没有新增行类型。

## Validation

- `openspec validate complete-tool-call-presentation --strict --no-interactive` — passed.
- `rustfmt --edition 2024 --check` on the two touched Rust files and `git diff --check` — passed.
- `cargo test --locked --offline -p shell --lib lsp_and_dynamic_tool_starts_keep_specific_identity -j 1 -- --test-threads=1` — 1/1 passed.
- The same freshly built Shell test binary ran `context_recall_and_dynamic_outputs_complete_their_tool_rows` — 1/1 passed. After the shared Shell changes stabilized, the rebuilt binary ran both tests again — 2/2 passed.
- `openspec validate --all --strict --no-interactive` — 20/20 passed before archive.
- `openspec validate --all --strict --no-interactive` — 19/19 passed after archive.
- `openspec validate --archived --no-interactive` — final rerun after the archive task was checked, 379/379 passed.
