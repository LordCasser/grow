# 实施验证场景与现有覆盖

以下是后续实现的验收计划，本轮未执行新产品行为测试。不得把前轮 408 项回归或临时数据修复探针当成本方案已通过。

## 必须新增或增强的场景

| ID | 触发 | 目标断言与反例 |
| --- | --- | --- |
| V01 | 主任务挂起，修改 AGENTS.md 后冷恢复 | 原 session/Turn/Step 继续；模型规则、classifier 规则、source 投影一致且为持久有效版本；不改 checkpoint hash，无额外用户输入或工具重放。 |
| V02 | 挂起后新增/删除 AGENTS.md，原任务没有项目规则或关闭 agents_md | 保留原本无规则语义；当前新文本不被偷偷注入，也不使规则解析不明时猜一个文件替代。 |
| V03 | 多级规则、多个 typed block、compaction 后恢复、rewind 旧分支 | 与实际有效模型投影相同的内容和顺序；不取物理日志最后一条，不复活不可见规则。用实际 public spawn/restore 验证，不只测试一个模拟 if。 |
| V04 | parent/child、fork 继承不同来源规则，共享权限 actor | child 不覆盖 root classifier；模型继承上下文不转化为新人工授权，fork 不继承原树执行资格；普通 fork/derived child 的规则选择分别按其契约核对。 |
| V05 | 仅改 Agent color、description 或正常目录文件 | 执行投影等价时恢复成功；selector/plugin 真变更仍精确区分；不得修改全局业务相等语义来通过此测试。 |
| V06 | 相同真实目录通过 `/tmp` 与 `/private/tmp` 加载 | 证明同一目录后通过；目录被替换、目标丢失、越过原隔离边界时仍受阻，不使用前缀替换证明身份。 |
| V07 | 当前工具/父 ceiling 收紧、当前配置更宽、原 MCP 成员事实不足 | 收紧不能扩权；下一工具仍按当前约束检查，允许恢复历史不等于授予执行；更宽配置不能增加 child 出生能力；原能力不可证明时在执行放行前拒绝。 |
| V08 | 模型 catalog alias 变化；effort、真实 endpoint/backend/model 变化 | 同实际路线 alias 可通过；effort 在控制边界生效；真实路由变更不得静默接受，不复用旧 native lane；key 轮换不被误称为 endpoint 变化。 |
| V09 | maxTurns/output grant 降低、提高；已有消费和 retry deadline | 不重置消费/attempt/deadline；更紧上限在派发前生效，提高配置不自动给旧 child 增发额度。 |
| V10 | Hook registry 改变，存在已完成/中断/未知 handler 或 Workflow 未结束 | 已完成 Hook 不重跑；在途 handler/Workflow 沿原执行源恢复，缺源明确拒绝；无关未来 registry 更新不因整份配置变化阻断原任务。 |
| V11 | 第一次 load 在 TreeSuspended 写入前失败，稍后同 operation 完整提交 | 同进程第二次 Retry 可按新事实完成；root-only fork 同样覆盖；新进程结果与同进程一致。缺少必要节点或 CommandsSettled 的反例不放行。 |
| V12 | 首次失败发生在 actor 已注册、尚无 RestorePrepared，修复依赖再重试 | 失败创建的 closed-admission actor 可安全退休并重建；没有 SessionEnd/child Cancel/FIFO dismiss；不终止别的 viewer 正常使用的 actor。 |
| V13 | RestorePrepared ACK 丢失、某 leaf 已 ExecutionReleased、并发 resume | 原 operation/instance 与已消费资格不重复；不能通过重建临时 actor重跑 leaf 或另造 task/prompt；保留真实终态与一次交付。 |
| V14 | 旧 writer 尚存或正在补事实；请求超时/断开 | 不开第二 writer、不虚报可恢复；原 owner 继续结算；下一次显式 Retry 不继承永久错误缓存，也不无限自动重试。 |
| V15 | 关闭唯一 owned root 页后只剩 viewer，再 Quit；两个 leader 客户端各有 root | 自己的所有 root 都挂起，另一个 owner 的 root 不受影响；最后一页可关闭落 Dashboard/Welcome，不丢后台执行责任。 |
| V16 | 删除普通 fork、超 16 MiB 长历史、真实结果链、未消费 checkpoint | 普通 fork 独立删除；长期历史不因观察预算永久拒绝；真实跨 session/Plan/恢复责任仍保护；已确认放弃原工作须有明确事实，不能伪造成功。 |
| V17 | 删除在 teardown 后磁盘失败；欢迎页 Stay 删除成功；fork copy/load 失败 | 本地绑定与实际 owner/存储状态一致，可重试；仅导航目的不同不跳过清理；复制成功但加载失败不误删已保存历史。 |
| V18 | 损坏 checksum、缺失原输入/accepted response/结果引用、伪造 parent、未知副作用 | 仍拒绝对应执行；不能为提高可用性跳过因果校验、用当前文件补造原证据或盲重放工具。只读查看有明确受阻信息。 |

V01/V11/V12 优先采用隔离 Grow home + 真实 MvpAgent public load + 受控 writer ACK 的确定性夹具；provider 使用受控 adapter，不连接线上模型。V15 用两个真实客户端验证 owner 路由。没有触发新分支时不重复全工作区构建。

## 已有可复用测试

- `shell/src/session/actor/tests/runtime_suspended_waiter_tests.rs`：`runtime_source_hash_is_64_hex`、`restore_source_drift_and_foreign_operation_leave_the_checkpoint_closed`。后一项必须把“foreign owner 仍拒绝”和“可接受规则/展示变化”分开，不能删掉整个失败测试。
- `shell/src/session/actor/tests/runtime_restore_tests.rs`：`accepted_final_response_resumes_original_turn_without_provider_or_prompt_replay`、`suspended_workflow_rebinds_the_same_open_child_before_original_turn_releases`。
- `chat-state/src/session_runtime.rs`：hash 格式、phase/operation/instance 顺序。若后续删除聚合 hash gate，格式测试随真实 schema 变化调整，不能拿它代替恢复语义测试。
- `chat-state/src/actor/tests.rs`：`root_tree_suspended_lost_ack_retries_one_exact_barrier_fact`、`blocked_or_full_tree_barrier_does_not_claim_success_or_replace_writer`、`runtime_cold_load_prefixes_preserve_turn_until_execution_release`。
- `pager/tests/pty_e2e/suspend_nested_task_tree.rs`：`ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree`、`cold_resume_after_partial_leaf_release_does_not_replay_checkpoint`、`concurrent_cold_resume_claims_one_root_without_replaying_tree_work`。
- `shell/src/agent/mvp_agent/tests.rs`：load guard、失败唤醒、disconnect replacement、lifecycle serialization、delete teardown、`reload_after_terminal_removal_starts_clean`。这些不等于已经覆盖“首次挂起 load 失败，事实后到，同进程 Retry 成功”。
- `shell/src/session/actor/tests/project_instructions_idempotence_tests.rs`：typed tag 与插入幂等；其中部分测试模拟 if，不验证真实 spawn 链。
- `shell/src/session/actor/tests/permission_auto_mode_tests.rs`：synthetic filtering、`agents_md_classifier_body_strips_system_reminder_framing`、`subagent_does_not_set_classifier_project_instructions`。还需新增模型/source/classifier 的恢复同源断言。
- 前轮 `audit-fork-multi-session-journey` 的 7 个失败断言可转为 A1/A4/A5/A6/A7 的回归起点；其 probe 源码保存在归档证据中，不在默认测试编译范围。

测试路径均相对于 `crates/codegen/`。实现前使用 `cargo test --locked -p <crate> --lib <filter>` 精确列举/执行，再按开发指南运行必要 PTY；不为了文档分析重新产生 Cargo 产物。
