# 验证记录

## 现场重放

通过只读文件句柄读取会话 `01a2028b-1dd8-7693-ba23-52d67ffebb2f` 的 Timeline 和三条失败 InfoRequest 的原始问题、冻结范围与准入预算。临时离线测试按各自的最后序号调用 `Timeline::from_events` 重建 Surface，用生产 `project_portable_history` 复算原请求，再调用本次 `FrozenQuestion::select` 和 `estimate_request_input_tokens`。测试断言原问题逐字保留、选择后仍有历史证据，以及旧输入超限、新输入在原上限之内。这三份 Surface 均未携带图片。实验没有运行原会话、发出模型请求或写入用户数据，仓库没有保留现场正文或专用测试入口。

| 冻结末序号 | 全历史输入估值 | 修复后输入估值 | 输入上限 | 保留的历史项 |
| ---: | ---: | ---: | ---: | ---: |
| 27208 | 1,798,845 | 1,045,571 | 1,048,576 | 1,686 |
| 27212 | 1,798,575 | 1,045,301 | 1,048,576 | 1,686 |
| 27216 | 1,798,270 | 1,044,996 | 1,048,576 | 1,686 |

## 场景与边界

- 协调队列保留最近的完整工具批次、原始问题、来源坐标和父会话 Surface。通过 mock provider 后核对成功终态、结果凭据与唯一 `InquiryAnswered` 收据。问题自身超窗时只留下 Request 和 Failed。请求等待 ACK 时外部取消，调用方取得取消结果。
- `/btw` 遇到 overload 时复用原选区，即使主会话在两次请求间前进。图像被明确拒绝后使用入选图片的冻结身份生成描述；描述膨胀超限按原预算拒绝，磁盘 ledger 中只有首次回答尝试。必需输入超窗单独核对零回答请求与失败诊断。
- Portable 投影覆盖全局重复 call ID、同批重复结果、部分完成批次、原下标与技能元数据重排。预算边界覆盖一个批次包含失效和有效调用、两个 SkillLoad 以及多个系统消息。完整输入能容纳时与原投影内容一致。
- 子代理真实工具入口覆盖不可审批的主动发送。提示引导通过最终答复交付。正常子任务结果引用、收件证明回复、权限判定和 peer 传输大小约束继续沿用既有实现。

## 测试环境与命令

macOS 本地运行。完整库测试使用 PATH 中的 Rust 1.99.1。Rust 测试使用仓库要求的 `RUST_MIN_STACK=16777216`，独立的临时 Cargo target、关闭增量编译和测试调试符号。问答由本机 mock provider 回答，图片回退同时核对物理 JSONL ledger。初次在默认小栈运行 Shell 队列夹具时发生测试线程栈溢出，按开发指南设置测试栈后通过。初版测试夹具缺少发送工具注册、观察端异步转发晚于 ACK；最终用例注册真实消息工具，并在 writer 返回确认前捕获侧问事件。

- `cargo test --locked -p sampling-types -p chat-state -p agent --lib -- --test-threads=4` 通过。Sampling Types 330 项，ChatState 679 项、忽略 3 项，Agent 504 项。
- `cargo fmt --all -- --check` 与 `git diff --check` 通过。
- `cargo check --locked -p cli --bin grow` 通过默认功能构建检查。首次构建 jemalloc 时，PATH 中的 OpenHarmony `diff` 对缺失文件返回 0，使 configure 跳过头文件生成。仅在该次构建中优先使用系统 `/usr/bin/diff` 后通过。
- 通过 rustup 使用仓库锁定的 Rust 1.93.1，重新编译并运行 Shell 的协调预算、选区、侧问和主动发送权限相关回归，18 项通过。该工具链的 `cargo fmt --all -- --check` 亦通过。
- `cargo test --locked -p shell --lib --no-run` 构建最终测试程序，使用 `--test-threads=4` 运行全部 Shell 库测试，4,889 项通过，14 项忽略，零失败。覆盖协调、权限、图像恢复、recap、memory flush、存储与重放。
- OpenSpec 归档前严格校验 19/19，通过归档后全量严格校验 18/18、历史归档校验 985/985。
