# 验证记录

## 定位与范围

- `pager/src/app/root/dispatch/notes.rs::dispatch_send_btw` 取当前 root Agent 的 SessionId，`shell/src/extensions/feedback.rs::handle_btw` 按同一 id 取 SessionHandle，run loop 调用其 `handle_side_question`。此次未改变 UI 或 session 路由。
- `chat-state/src/actor/mod.rs` 的 `MaterializeTimeline` 在单命令内返回当前 Surface 与 high-water ref。旧 `handle_side_question` 在物化后连续 pop 全部尾部 ToolResult / Assistant(tool_calls)，没有按结果匹配判断完成状态。
- 原始截图未附请求 artifact；没有回放用户那次设备会话，也没有访问或修改 Fedora 设备。本次证明的是源码中的确定性证据丢失及修复，不把 loopback 固定回答当作真实模型语义正确性的证明。
- 独立发现的 recap 尾部裁剪登记 `openspec/backlog.md`，没有混入本次实现。

## 修复前复现

命令：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib side_question_preserves_completed_tool_evidence_on_all_backends -- --test-threads=1`。

首次编译发现测试夹具循环移动了非 Copy 的 ApiBackend，改为 clone 后完成编译。保留旧生产逻辑运行新测试：**0 passed, 1 failed**，失败断言为 `ChatCompletions: lost deployment evidence with 2 verification results`。fixture 中一个部署调用和两个核对调用均有真实 ToolResult，但旧逻辑把全部尾部证据删掉，只剩“尚未部署”的旧正文。

## 场景与验证入口

| 场景 | 验证 |
| --- | --- |
| 连续完整工具尾部 | `side_question_preserves_completed_tool_evidence_on_all_backends` 的 completed=2，检查实际 HTTP body 中部署/核对正文、调用与结果身份及文本/图片附件 |
| 部分并行调用完成 | 同测试 completed=1，保留 deploy/verify_a，wire 中既无 verify_b 调用也无伪造结果 |
| 最新调用均未完成 | 同测试 completed=0，保留 deploy 与最新 Assistant 正文，不输出 verify_a/verify_b 协议 |
| 三种 backend | 上述三场景逐一经过 Chat Completions、Responses、Messages 真实 loopback 请求，共 9 种组合 |
| 主 Surface 隔离 | 每次请求前后按序列化值比较主上下文，旁路问题/回答不进入主 Surface；请求没有 tools/tool_choice |
| 重试快照一致性 | `side_question_retry_retains_frozen_tool_evidence` 在 provider barrier 暂停第一次 overload，推进主 Surface 后释放；比较两次完整 wire body 和 durable input/source refs |
| 原有行为 | 运行 recap / side question 相关 shell 回归与 sampling-types 全部单测，覆盖配置快照、reasoning、温度、协议转换及重试策略 |

## 修复后结果

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib recap -- --test-threads=4`：**63 passed, 0 failed**。包括两个新增真实 wire 回归及原有配置快照、重试、recap、suggestion 生命周期测试。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p sampling-types --lib -- --test-threads=4`：**278 passed, 0 failed**；包含 6 项更新后的 btw portable projection 测试。
- `rustfmt --edition 2024 --check`（三个变更 Rust 文件）与 `git diff --check`：通过。
- `openspec validate --all --strict --no-interactive`：归档前 **18 passed, 0 failed**，归档后 **17 passed, 0 failed**。
- `openspec archive fix-btw-context-tool-evidence --yes`：已归档并合入 model-sampling 主规范。归档时仅最终归档校验任务保持未勾选，完成归档后的严格规范校验才勾选。
- `openspec validate --archived --no-interactive`：**338 passed, 0 failed**。

macOS shell 测试链接器报告大型 `__eh_frame` 超出 compact-unwind 编码范围的警告，修复前后均成功链接。没有构建或安装新的 Grow CLI，没有替换运行中的二进制。
