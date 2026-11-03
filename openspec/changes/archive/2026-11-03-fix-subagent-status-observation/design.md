## Evidence

- `task/coordinator/query.rs::active_inspection` 在 progress_snapshot 为 None 时返回 Unavailable；这个不虚构计数的边界保留。
- `shell/agent/subagent/handle_request.rs` 初始化 None，且仅在 !restoring_open_turn 时启动 publisher。原 Turn 恢复因此始终无样本。
- `spawn_progress_publisher` 在更新缓存后等待 gateway completion；网关阻塞同时冻结后续本地样本。
- `task_output/mod.rs::observation_unknown_result` 将查询失败投影为 running，结构化状态与正文的 unknown 冲突。

## Decisions

用户会话证据：截图前的查询（Domain seq 13250/13256）询问 Rust F9-1 `01a19f7c-8dbb-7a40-a0e0-8ee35eb79533` 与 TS F9-2 `01a19f7d-6300-7043-b6ba-135b8eb5b13b`，都返回 unauthorized；父 Timeline seq 11792/11794 已分别记录 cancelled 和 canonical Result 引用。原始会话只读，不复制任务正文或修改其记录。

ChannelBackend 保留唯一查询入口，Shell 在统一 Agent 构建处为其绑定只读历史查询。仅在 coordinator 无记录的响应后，使用剩余原期限核对当前 source、lifecycle root 的 Spawn、直接父级/根所有权及 child Result 链。root 与 immediate parent 权限沿用现有 belongs_to_session 规则；Workflow 隐藏 child、无关来源与未知 ID 不暴露存在性。读取复用 bounded_shared_read 的 worker 许可和 shared/pinned storage，不创建 writer、不恢复执行、不写完成缓存。找到了授权 Spawn 但没有 canonical terminal 时仍报告观察未知。

复用现有 signals actor、child control 的快照和单个 publisher。先取得真实 signals 样本再接入 coordinator；随后在发布 child 身份后、等待初始控制 ACK 之前启动 publisher，普通与恢复共用该入口。无 signals 时继续保留 None，不以默认零值补齐证据。

publisher 在同一 select 循环中处理采样时钟、取消和一个可选的网关 completion。通知未完成时只更新本地缓存，不排队其他通知；交付恢复后下一采样 tick 发送当前值。保持现有心跳、去重和取消所有权，不新增重试或超时。

采样器的 child cancellation token 由原 runner 作用域持有 drop guard：父取消继续传递，runner 返回或恢复挂起时停止该次采样器，不反向取消任务或产生第二个残留采样器。

工具将 typed 观察错误分别映射为 observation_unavailable、observation_timed_out、observation_invalid_evidence；这些都不是终态。多项等待仍保留原待观察身份，授权失败仍独立处理，不泄漏其他来源。

真实 parent 日志约 49 MB，超过诊断接口 bounded_validated_timeline 的 16 MiB 扫描额度；普通 validated_timeline 还会核对所有 prompt/模型产物，实际探针在五秒期限内无法完成。本查询复用现有删除资格检查的 canonical reference 验证，将原 deletion_timeline 更名为 reference_timeline 供同一只读职责复用：仍校验 journal、因果、展示结构与 owner，并进一步验证目标 Spawn/Seed/Result 和适用输出 artifact，不加载无关通知正文、Sideband 或模型产物。有界性由原调用期限、查询许可及底层只读 worker 许可保证。超时的 reader 继续持有 worker 许可至实际退出，不扩充额度或新增配置。

已加载 source 的权威事实直接从原 ChatState owner 读取，复用 AgentRebuildSpec 保存的同一 handle；不重新解码整个 resident 日志。读取仍在原查询总期限内，ACK 阻塞时可返回超时；验证在原 blocking worker 进行。只有 nested source 的非 resident lifecycle root 和目标 child 使用 canonical reference 文件读取。没有第二份生命周期缓存。

## Verification

先在旧实现复现网关阻塞时缓存不更新及工具错误状态被写成 running，再验证修复。通过真实冷恢复用例查询原 child，覆盖恢复中未结束 Turn 的状态。复用现有 coordinator/backend 回归验证授权、真实终态和观察未知边界；不依靠工作区文件证明完成。构建复用现有 target，仅清理本次新增的编译残留。
