## Reproduction

修复前生产 ChatState actor 回归：跨压缩 rewind 的 portable 正文估算为 428 tokens，但显示为 40,299；另一路由替换回归中 portable 正文为 300，却保留旧 provider 的 50,000-token 锚点。两项均实际运行并观察失败，不依赖静态推断。

## User session evidence

只读检查用户提供的 `01a16811-f823-7a52-8b64-9a3ab105151a` 的本地 Timeline，没有改写会话、调用 provider 或运行历史工具。相关北京时间顺序为：

- 2026-10-29 09:04:22，切到 `neorouter/claude-opus-4-8`（Messages）。
- 09:05:50，切到 `neorouter/grok-4.7`（Responses）。
- 09:06:22，domain seq 29284 的 rewind 完整恢复 4,246 个 Surface item，其中 1,007 个 reasoning；此前存在三次历史压缩。
- 09:07:55，已有自动压缩又提交了一次局部替换。后续请求报告一般性的 HTTP 400 invalid_request；该错误没有明确超窗证据，本次不将它直接归因为上下文大小。

用本次构建的 `chat-state` / `sampling-types` production library，在临时只读诊断程序中反序列化该 rewind 的完整 `ConversationItem`，调用现有 `estimate_conversation_tokens` 与 `project_portable_history`：

| 项目 | tokens |
| --- | ---: |
| 原始 rewind Surface 估值 | 3,229,126 |
| retained reasoning 贡献 | 1,984,367 |
| 默认 portable replay 估值 | 1,244,759 |

因此该会话确有原始 reasoning 被错误计数的问题，但 portable 正文本身也超过截图中的 500K 窗口。本次不截断真实 overflow，也不删除、重写原会话或私有正文。计数不是 provider 的精确 tokenizer 测量，工具/schema 等实际 envelope 仍由首次请求添加。临时诊断产物已自动删除。

## Validation

- 使用项目指定 rustup Rust/Cargo 1.93.1，避免 PATH 中 Homebrew 1.99.1 的差异。
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib -- --test-threads=4 --quiet`：最终 579 passed，0 failed，3 existing ignored。
- 跨压缩 rewind + 三种 backend 切换覆盖 retained reasoning、真实 portable overflow、即时唯一 pressure 事件、完整历史保留、新消息增量与重复请求投影。
- 路由回归覆盖普通参数更新保留锚点、真正路由替换立即重新估算且无新增 Timeline 写入、随后 provider anchor 正常校准。
- 同路由 learned Chat reasoning 回归确认 rewind 后需发送的 reasoning 被计入。补查时先发现 false include_reasoning 导致 300 / 20,300 的失败，修正为与现有请求估算一致的规则后全套通过。
- 持久化 ACK 故障回归确认未确认及失败重试期间不发布 reset，最终成功后才重建计数；无效 rewind target 保留原 provider anchor。已有 compaction signed delta / overhead 测试全部通过。
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p chat-state --lib` 通过，确认正常 production library 构建。
- `cargo fmt --all -- --check`、`git diff --check`、归档前 `openspec validate --all --strict --no-interactive` 通过（17 items）。

独立复核发现的非 rewind signed delta 饱和组合债务只登记 backlog，未扩大当前改动。现有其他未提交改动保留。本次未构建或安装 CLI，没有真实终端/provider 交互回归；状态栏沿现有 Shell ContextPressureUpdated 通知链更新。

## Archive

`openspec archive fix-rewind-route-context-pressure --yes` 合入一项新需求。归档后 `openspec validate --all --strict --no-interactive` 通过（16 items），`openspec validate --archived --no-interactive` 通过（713 items）。本次生成的本地 Cargo target 在构建停止后清理；任务开始时该目录不存在。
