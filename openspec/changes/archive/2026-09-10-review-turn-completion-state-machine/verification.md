# 验证记录

2026-09-10，本轮仅改审查资料与 backlog，不改运行时行为。

- 阅读 model-sampling、session-timeline、input-admission、behavior-goal、context-compaction 契约与相关归档设计。Atlas 查询为 partial/closure_boundary，实际结论回到已定位源码确认。
- 复用 `target/debug/deps/shell-04653512b14be20c`，筛选 model_switch、explicit_completion、final_steering_fence、4 个异步压缩边界和 goal_degradation：**40 passed，0 failed**，5.38 秒。
- 复用 `target/debug/deps/chat_state-5ae981b5283cffd5`，筛选 continuation、partial_compaction、portable、tool_result_durably：**11 passed，0 failed**，0.05 秒。最终选择了哪些测试以日志为准，筛选串匹配不到测试不代表该场景已覆盖。
- 测试日志：`/tmp/grow-completion-state-review-shell.log`、`/tmp/grow-completion-state-review-chat-state.log`。
- 缓存二进制来自前次完成协议验证，未重新编译当前整个 HEAD；dad7f053 到本轮基线的额外变更位于 storage 观察打开路径及相关 fixture/changelog，不在这些完成/切换分支。缓存回归不是对整个当前 HEAD 的重新构建认证。
- 新增 `probe.rs` 调用真实 adapter/request encoder；`python3 run_probe.py` 根据 Cargo fingerprint 选择同一构建的依赖，直接链接现有库，不重新构建 workspace。六个拒绝/截断组合确认 ToolCalls 覆盖并保留 FinishTurn；第七个反例确认 Messages 目标 ID 碰撞。日志：`/tmp/grow-completion-state-review-probe.log`。
- 探针首次手工选择最新依赖时出现 serde_core/futures_core 构建身份不匹配，属于链接选择错误，未执行产品场景。改用 fingerprint 精确匹配后，编译和全部探针成功。
- 临时探针可执行文件 **16,787,600 bytes**，TemporaryDirectory 已自动删除。没有新增 Cargo target/incremental 缓存；磁盘检查约 **14 GiB** 可用。未删除用户会话、已有编译产物或其他任务文件。
- 没有访问真实 LLM 端点，没有新的 Shell/Goal 全链路反例构建。审查报告已分别标记动态确认、源码推演和能力风险；51 项既有回归通过不能否定新交叉问题。

归档前 `openspec validate --all --strict --no-interactive`：20 passed。审查以 `--skip-specs --yes` 归档；归档后全量 strict：19 passed，归档 strict：314 passed。日志分别为 `/tmp/grow-completion-state-review-spec-pre.log`、`/tmp/grow-completion-state-review-spec-post.log`、`/tmp/grow-completion-state-review-spec-archive.log`。归档路径、探针 root 解析和 Python 语法已核对，`git diff --check` 通过。
