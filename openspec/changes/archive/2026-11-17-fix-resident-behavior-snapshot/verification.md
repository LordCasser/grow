# Resident Behavior快照验收

## 修改前真实失败

- v2.6.17候选0286920e96f9d4c3e09d2aaad594aacceb7786a3官方run43943122333，Windows native job133565868315在scripts/test_local_coordination.py:473失败：`session/load` code-32603，data=Session is unavailable for a confirmed Behavior snapshot。此前库和默认CLI编译通过，脚本前8类跨进程检查通过。
- 同run Linux aarch64分发job133565867716编译成功，但实际release binary的同脚本同473行同错误失败，因此不是平台编译问题。
- 父修改前本机原脚本task01a1e5fe-6a93-75a0-a4d2-c9f14aa5bb91退出1（17.26s），在同473行返回完全相同错误。binary是已验收最终产品源码、metadata2.6.16；未混入本次修复，原断言无改动。
- 三处触发均为model.block=true、目标resident actor回答sideband期间load；末尾session-end writer unavailable日志发生在脚本finally清理，不当作load之前的持久化故障证据。

## 根因与验收计划

父亲读ChatState actor::persist_pending_timeline_event：每次健康append开始就设Some，ACK后清None。Shell actor和MvpAgent复核将Some等同不可用，超出Behavior自身Control确认边界。新修复不改通用writer，而收窄这两处观察判定；确定性hold-ACK旧红新绿、原Control失败与确认等待、真实跨进程整链由实现后追加。

- 确定性旧红：task01a1e606-9a0d-74c1-8f80-97fc96b194ad exit101；测试`healthy_non_control_ack_does_not_block_confirmed_behavior_snapshot`实际执行0.11s，在健康Observation ACK被hold时收到原snapshot unavailable错误（0pass/1fail）。该次生产guard未改。首次夹具SamplingConfig类型编译失败已修正，不计作旧红。
- 生产候选最小修复已落地：Shell SessionActor共享私有域规则判断closed、pending Control、error_kind/error_message或acklost；actor与MvpAgent回包复核共用，只放行健康非Control append。双gate、execution_open、samechannel/epoch核对保留；不改通用ChatState writer、不加重试。父已亲读diff。
- 新CLI默认构建及原完整协调脚本task01a1e609-a829-7a72-8896-f87f4c3c77b7整体exit0（146.39s，其中构建124s）：原失败resident reload已通过，并继续通过running/queued crash receipts恢复。脚本10类PASS，原473及所有断言未改。修改前同脚本exit1，新CLI只含本项产品修复加版本metadata递进，构成真实链路旧红转绿。

- 定向绿色：task01a1e609-b04f-7981-890b-a55c04049475整体exit0。session_mode完整12 passed（含既有failed_control_write和snapshot_during_ack、healthy held ACK成功、fault/acklost/closed/execution_closed/suspended拒绝）；真实behavior_snapshot_from_actor直接测试1 passed，覆盖healthy/control/fault/acklost/closed/actorchange/epochchange。fault后重试ACK也被hold，反例不依赖25ms竞争窗口。
- 父亲读三文件生产diff和held ACK测试，确认ACK释放后快照值不变、Control数不增；共享helper归Shell当前owner，不改变ChatState通用writer。实现子交付01a1e602-4185-7272-be4e-92abdc36eb5c已接受。
- 新产品源码正式release-dist -Dwarnings check与workspace all-targets均通过（父task01a1e60c-867b-7e72-a5f3-0ea910c6d3cc整体exit0）。全仓fmt、diff空白、变更链接/锚点和OpenSpec all strict均通过。开发者说明同步健康pending与Control确认的边界。
- 父验收完成；这只确认本项最小修复及本机真实链，新的正式候选仍须重跑同SHA的跨平台原生/分发门禁。旧v2.6.17失败结果不因局部通过而改变；其tag后按用户明确指令撤销，原SHA/run记录保留，详见对应发行归档。

只读独立调用序核对：目标sideband HTTP发出前的Spawn/Request flush、Attempt usage和request evidence均已ACK，故不能将473瞬间pending具体归因于这些已结束写入。load已完成PauseForReplay、PublishControlState与PublishCoordinationState，错误来自actor快照OR判断，不是caller的actorchanged错误。日志未区分三条件；健康非Control pending误拒是静态已证缺陷，只有确定性旧红新绿加原脚本新CLI通过，才能验收其对实际失败的覆盖。不把teardown日志或特定pending来源猜测作为根因证据。
