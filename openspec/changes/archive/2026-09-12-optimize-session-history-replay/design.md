## Decisions

1. 使用现有 RawLinePeek 查看方法，只对 Grow payload 做 typed decode。继续跳过坏的缓存行，保留通知 sessionId、eventId 与 meta；Timeline 的严格验证不受影响。两个 Grow replay 入口共用一个扫描实现。
2. 128 行分批 drain 的实测完整加载变慢，因此撤回该修改。既有发送顺序、pending_tool_calls 生命周期、最终完成屏障和 offset delta 同步 gate 顺序保持。
3. 快照读取单独计时；性能解析器累加同名阶段。样本使用前一 change 修复的 fixture，128/512 turns，8 chunks/turn，4096 bytes/chunk。吞吐测量不启用额外历史收集。
4. 独立启用 GROW_PERF_ASSERT_HISTORY，对照 typed replay 在 load response 前比较全部 user/assistant 通知的顺序和内容。PTY 没有可靠的 loading 完成控制钩子，本轮用 reducer 检查加载期间输入/队列语义，现有 PTY 验证恢复历史和后续提交；不声称证明真实键盘回显 p95。

## Validation

Grow-only 流与原 typed reference 在混合方法、rewind、坏 payload、部分尾行、通知身份上等价。运行 shell/pager 回归及相同 fixture 性能测量；编译串行、复用 target、检查磁盘。
