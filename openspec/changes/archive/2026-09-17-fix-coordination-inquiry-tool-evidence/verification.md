# 验证记录

## 原事件事实

只读核对截图中的三个本地会话，根目录为 `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fjarde/`。没有修改会话、运行中的配置或二进制。

| 会话 | 原始记录 | 事实 |
| --- | --- | --- |
| R：`01a0ae9c-3fd4-7d11-9dfe-789b2d428841` | `timeline.jsonl:518`（seq 517），`:531`（seq 530） | `ask_parent` call `call_00_9qfqkCIXZZfQct4yiX8V8499` 询问接口、计费和测试位置；19,691 ms 后 success。 |
| D：`01a0ae9c-86f7-7a90-bfdc-551c5087ce41` | `timeline.jsonl:491`（seq 490），`:501`（seq 500） | `ask_parent` call `call_01_kc6hsU0PGyJtM7F7JpI75111` 询问 serde schema 取舍；77,187 ms 后 success。 |
| parent：`01a0ae68-474d-73b1-94fd-878274dec03b` | `sidebands/01a0aea1-f554-7932-b7ad-36a912cff0c6/timeline.jsonl:3` | 回答 R 的接口问题，部分内容无法确认，提供计费/测试建议。 |
| 同 parent | `sidebands/01a0aea2-ca46-7ed3-8065-3e4b5050b9a7/timeline.jsonl:3` | 回答 D 采用 struct variant、字段名 value。 |
| 同 parent | `timeline.jsonl:1579`、`:1584`；完成 `:1593`、`:1598` | 随后真实发起两次 ask_subagent 追问。 |
| R / D | 各自 `sidebands/01a0aea6-1416-7021-a772-e5d998573197/timeline.jsonl:3`、`sidebands/01a0aea6-1430-7e00-994b-d4738e685b3c/timeline.jsonl:3` | 回答可见上下文没有 ask_parent，和先前成功调用的 Timeline 事实矛盾。 |

三个会话的主 Timeline、updates 和 Sideband JSONL 未找到 PermissionJudgment；这里是显式模型问答。工具调用的普通 allow 预检并不等于自动权限升级。协调询问是 InfoRequest，真正的自动权限桥接是独立 PermissionJudgment，授权输入仅来自其专门策略与真实用户输入。

两个追问 Sideband 的 source_refs 分别覆盖 seq 0..799 与 0..574，包含先前调用的 seq。source_refs 只证明声明的冻结来源范围，不证明实际 wire 保留了所有消息；`evidence_refs=[]` 也不能用来判断请求漏掉了工具。本次故障归因结合原始成功记录、矛盾回答、源码删尾算法和真实请求复现，不能仅凭模型自述断言工具未发生。

进一步检查这两个 Sideband 目录：只有 timeline 与 lock，attempt 保存 refs、assembly manifest 和 token 数，没有 request/wire artifact。因此没有直接取得原线上请求正文；本次可确定的是显式询问已成功、追问回答失实，以及旧代码在等价尾部结构下确定性漏掉已完成调用。

## 回归与实现

- 新增 `inquiry_preserves_completed_tool_evidence_on_all_backends`，经真实 `handle_coordination_inquiry` 和 loopback provider 捕获实际 wire；三种 backend × parent-to-child/child-to-parent/peer × 完整/部分/全部未返回尾部，共 27 种组合。
- 断言完成 ask_parent 的问题与回答、后续完成结果、Assistant 正文和图片附件保留；发送调用与结果身份严格匹配，未完成协议不发送；无 tools/tool_choice，仅一次请求，主 Surface 不变。
- 第一次测试编译发现夹具缺少 inquiry 类型 import，修正后运行旧生产实现，失败于 `ChatCompletions/ParentToChild/2: lost completed ask_parent arguments`。这是模型输入证据失败，未依赖 mock 回答的自然语言内容。
- 替换为既有 `project_portable_history` 后，`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib coordination -- --test-threads=4`：42 passed。覆盖新请求用例、FIFO/取消、跨工作区审批、持久恢复、实时重连及进程间协调。
- macOS linker 报现有测试二进制 unwind section 超过 16 MiB 的 warning；编译与测试成功。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib permission_auto_mode_tests -- --test-threads=4`：18 passed，包含真实 child classifier 请求、拒绝模型/工具/合成摘要作为授权输入和主 ChatState 隔离。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib subagent_bash_permission_tests -- --test-threads=4`：9 passed，包含真实 child allow/deny/timeout、不扩张创建能力、in-fence 跳过 prompt 和拒绝后继续处理。

## 联合架构核对

用户要求后，与「修复 btw sideband 上下文获取」任务双向核对，确认独立快照、共享协议投影、消费者预算、Sideband 生命周期和专门授权来源的职责；见 design.md 的 Joint architecture review。对方负责 `/btw`、recap 和压缩，本 change 只修改 coordination.rs、local-coordination 说明及对应规范。未替换其已有工作树改动。

对方已完成并归档，记录见 [fix-compaction-budget-recovery](../2026-09-17-fix-compaction-budget-recovery/verification.md)。对方报告 chat-state 工具函数 136、shell compact 136、recap 63 项通过；本任务未重复执行这些已通过的无后续变更测试。memory flush 的 `snapshot_memory_flush_state` 仍使用 simplified 输入，已由对方登记 backlog。

## 规范收尾

归档前 `openspec validate --all --strict --no-interactive`：18/18 passed。`git diff --check` 与本次 Rust 文件的 `rustfmt --edition 2024 --check` 通过。场景逐项对应真实请求测试中的完成尾部与部分/未完成批次；主 Surface 隔离由同一测试和现有 busy-parent 测试共同验证，权限来源隔离由上述既有权限回归验证。

`openspec archive fix-coordination-inquiry-tool-evidence --yes` 已合入 local-coordination 主规范；归档后全量严格校验 17/17 passed。归档命令时仅规范收尾项尚未勾选，完成后再记录并校验归档完整性。

最终 `openspec validate --archived --no-interactive`：341/341 passed；`git diff --check` 通过。本任务未执行 Git 提交或安装。

尚未构建安装 CLI，不声称已经改变正在运行的 Grow；loopback 测试验证实际请求，不保证任意真实模型的自然语言答案一定正确。
