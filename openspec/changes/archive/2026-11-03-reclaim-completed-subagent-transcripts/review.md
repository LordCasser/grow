# 对抗性审查

主代理审查状态归属，Luna 子代理对前端实现进行独立只读审查，随后定向复查修正。审查没有代替运行验证。

已处理的发现：

- 已卸载 child 的迟到 ACP duplicate 原本会先读完整历史再去重。现在保留原高水位，并在恢复重状态前拒绝旧事件；历史回放后的第二道去重仍保留，用于处理 canonical/live 重叠。
- 历史读取失败不能设为 loaded，也不能追加会阻挡下一次回放的终态 footer。失败显示固定身份的单条 notice，实时正文不会先占据恢复位置。
- missing session 被既有 replay API 表示为 Empty。对已完成、没有有效正文的 child，Empty 现在保持可重试；新增移走整个源目录再归还的回归。
- 独立审查发现成功恢复后旧错误提示残留。成功回放现在删除该固定 notice，损坏/缺失源的恢复测试均检查提示消失。
- 主代理补查迟到 Finished：卸载后 helper 恢复终态、外层 Finished 再终结一次会重复 footer。终态展示现在使用已有 immutable event ID 去重，回归连续投递迟到 Finished 并验证 footer 只有一个。
- 主代理补查后台任务：旧 child history stream 过滤 TaskBackgrounded/TaskCompleted，会丢失先前 live 展示的行。已转发这两类持久事实并提取原 background reducer 供 live/replay 共用；原任务 stdout 和 owner 保留、行链接重建，canonical 终态仍能收敛同一 Running task。迟到后台生命周期先恢复历史，并按已有 task 行去重。
- 独立补查后台完成的迟到时序：若原 owner 仍 Running，不能一概禁止历史 Completed 更新状态后又将 live Completed 当作已有行丢弃。现在 canonical Completed 可以终结同一 Running owner，已终态 owner 不被回退；回归让非空 stdout 仅来自 live MonitorEvent，持久与晚到 completion snapshot 的 output 均为空。

所有权核对：保留原 AgentView、模型/行为控制、草稿和 live highwater；回收不执行 clear_transport_interactions、runtime Cancel/Shutdown 或历史删除。selected、running、replay/reload、viewer/interaction 及未确认 preview 的展示仍保留。纯展示的 edit highlight 和媒体加载使用既有失效入口；进行中的 Mermaid 用户操作不随 transcript 清理取消。Normal/Minimal 的共用事件循环调用同一 reducer。

`with_prepared_history` 完成 journal 校验和 history prepare 后才向 child 展示发送更新，因此本轮故障回归不是将半份 journal 当作成功历史。重复失败用现有 immutable event ID 去重，不建立新重试墙或诊断队列。

容量路径由主代理交叉审查：发现 completed cache 淘汰后 task ID 可以绕过 retiring 去重并覆盖旧 owner；已要求子代理补 guard 和真实 ChannelBackend 回归。该项验证记录留在独立 capacity change。

静态定向复查未留下已知未处理的本轮生产问题。50G 峰值的事故根因仍无完整历史分配证据，本轮验证针对已定位的两条路径。
