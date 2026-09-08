# Cancel rewind 审计

## 实现证据
- 默认开关从 Shell 经 ACP 进入 Pager `cancel_rewind_enabled`，不是闲置字段。
- Pager `dispatch/turn.rs`：shared queue、pending prompts 非空、native scrollback 已 committed、有较新的文本或图片草稿时，不做 pristine rewind。满足条件时恢复 stashed text/chip/images，移除合并段落，finish_turn 清 current_prompt_id，同时发送 rewind_if_pristine。
- 同文件实际队列逻辑保持 shared queue，由服务端推广 next prompt；旧注释误称将队首恢复 composer，已修正，仅注释变化。
- `dispatch/prompt.rs::handle_prompt_response` 从成功响应 meta 或错误的本地 prompt_id 获取身份，过滤不属于当前输入的响应。
- `agent_view/session.rs` 与 `acp_handler/mod.rs` 使用有界 rewound_prompt_ids 抑制被回退输入重新激活。
- Shell `actor/tasks_cancel.rs` 自行检查 state.rewindable，不仅信任 UI hint。前后台命令终止、Goal/子 Agent 处理仍是独立取消路径。

路径均在 `crates/codegen/` 下。自动取消回退与用户显式编辑历史的 rewind 流程不能视为同一个状态机。

## 测试及限制
现有测试包括 newer draft 不覆盖、combined segment 一并删除、minimal committed block 不回退、图片恢复重发，以及 late prompt response 隔离。测试命名不代替断言，已读取相关测试中的实际状态/文本/scrollback 断言。
本轮不声明完整跨进程竞态都已覆盖；UI 无活动不等于服务端必然 pristine，服务端仍作独立判断。没有操作用户真实会话或附件。

## 实际测试结果
命令使用 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib：
- cancel_rewind：3 passed。
- cancel_：75 passed。
- rewind：65 passed。
三次均 0 failed/ignored；过滤器可能重叠，不合计为独立用例总数。包含新草稿保留、图片重发、队列保留、晚到响应及显式历史回退相关测试。已有 macOS linker unwind table 警告，测试正常完成。初次构建耗时，始终等待原运行句柄，没有重启同一构建。后续测试有增量链接。

本轮未新增已确认运行 bug；注释修正不改变队列或回退语义。git diff --check 通过。
