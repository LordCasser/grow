## Root cause evidence

对会话 `01a15403-e361-7992-beff-a0f5db47ed4a` 仅做只读核对，没有执行、恢复或改写用户 session。seq2978 的原始 assistant 正文为空；seq2995 是成功的 description 投影。seq3031 的主请求含 350 个 wire messages，其中 12 条 assistant 正文精确为内部占位句。seq3032 的 HTTP 200 原始 SSE 完整结束，内容复述该句；seq3035 与 updates.jsonl 将其作为真实回复接纳。图片描述本身没有失败。

源头是图片工具与 carrier redactor 用自然语言替换原正文。旧单测直接断言该占位句，保护了错误语义；只测成功转换或 UI 操作行无法发现后续主请求受污染。新回归检查最终协议请求以及实际共用 TUI 块。

## Verification

环境：macOS arm64，Rust/Cargo 1.93.1，`RUST_MIN_STACK=16777216`，独立 `CARGO_TARGET_DIR=/tmp/grow-image-marker-target`，`-j 6`。

- 红测：新增测试配旧生产实现，`cargo test --locked -p sampling-types -p pager --lib image_projection --no-fail-fast -j 6`：6 项失败、2 项通过。失败覆盖 redactor 制造正文、direct Chat 压缩请求污染、native span 未 fallback、TUI 完整/分块/deferred 占位展示；正常解释的保留测试通过。最初测试代码有一处 `String` 与 `&String` 比较类型错误，修正后才执行上述有效红测。
- 绿测：四库同一 filter (`-p sampling-types -p chat-state -p shell -p pager`)：21 项通过，无 ignored。
- 完整核心库：`cargo test --locked -p sampling-types -p chat-state --lib -j 6`：312 + 568 项通过。ChatState 两项原有人工 benchmark 被忽略（frozen timeline replay / online append cost），非本次功能场景。
- 子 Agent：`cargo test --locked -p shell --lib resolution::context::tests -j 6`：45 项通过。
- 共用 assistant 块：`cargo test --locked -p pager --lib scrollback::blocks::agent::tests -j 6`：16 项通过。包括两个标记的所有两块切分点、deferred、部分前缀完成、分叉、精确构造、raw/pretty/copy、finish 重入和晚到追加。
- 真实 ACP 展示路径：`cargo test --locked -p pager --lib acp::tracker::tests -j 6`：164 项通过。新增 live/replay -> finish_all_running -> late chunk 回归覆盖子 Agent finalize 不清 tracker 的交错。
- 新 Timeline 回归在 Description、LocalOcr、UnsupportedModel 三分支比较 live/cold Surface 与身份；保持原始路径/图片证据，模型请求不含路径或内部 assistant 话语。三协议 wire 回归保留 tool call/result 关联；User、ToolResult、包含标记的解释和前导空格内容不被通用过滤。
- 上述验证的编译日志没有 warning/error。`git diff --check` 通过。

## Integration and boundary

隔离工作树先验证后，将本次六个源码/文档文件及 change 的精确差异应用到 main 工作目录。原有未提交 tracked 改动通过内容摘要核对保留，开发文档移除本次新增段后与原内容完全相等。原始 Timeline/provider evidence 与 canonical response projection 的 digest/updates 算法未修改，没有执行或改写用户会话，也没有改 MCP。

main 相同四库 image_projection 集成回归：10 + 6 + 4 + 1 = 21 项通过，0 ignored，编译无 warning/error。覆盖当前 main 工作目录的其他未提交实现与本次改动一起编译和运行；未将其他实现纳入本 change。归档前全量严格 OpenSpec 校验 17/17 通过。已清理本次独立 Cargo target，回收 37,039,773,271 字节（约 34.5 GiB）。归档后 `openspec validate --all --strict --no-interactive`：16 passed, 0 failed (16 items)；`openspec validate --archived --no-interactive`：635 passed, 0 failed (635 items)。

本次没有重新调用真实 provider、运行原生 TUI/PTY 或 Windows/macOS/Linux 发布矩阵；编译和协议/展示测试在上述本机环境执行。也没有改变 headless API 或原始导出证据的内容。未发布或安装新二进制。
