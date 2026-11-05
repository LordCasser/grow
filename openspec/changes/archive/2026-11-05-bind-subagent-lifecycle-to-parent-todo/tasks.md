## 1. Contract

- [x] 1.1 核对LocalPlan/Task/请求构造/通知及cold身份，记录可选绑定与MemoryContext偏差。
- [x] 1.2 确定单一计划owner、只读工具schema、固定stop/replan、数据/来源/交付边界。
- [x] 1.3 strict校验proposal/design/deltas后按owner交接首个独立增量。

## 2. Implementation

- [x] 2.1 ChatState提供独立request-only runtime context，Shell不再把计划合入持久memory。
- [x] 2.2a tool-types只读snapshot/query/schema与统一有界分页纯投影，冻结消费者接口。
- [x] 2.2b Shell自身plan owner Read，无计划与不可用可区分，不提交事实。
- [x] 2.2c tools session-bound backend/coordinator只解析真实直属parent与本次task/step，保留不可用错误。
- [x] 2.2d Shell runner读取该直属parent的同一ChatState快照，核对精确delegation身份。
- [x] 2.3 注册原生todo_read与类型/权限/输出转换，验证无写副作用。
- [x] 2.4 普通spawn/resume强制当前TODO绑定，首请求/每请求注入父项位置，移除prompt拼接。
- [x] 2.4a Shell每请求注入直属父项与本次委派身份，renderer满足UTF-8总字节预算（8 KiB），优先关联项并保留完整读取指引；用多字节步骤与长link/replan验证。
- [x] 2.4b Shell Link owner在同一gate以当前canonical scope检查session、有效step状态和admitted turn，验证edit/remove准入竞争。
- [x] 2.4c 普通TaskTool输入与schema强制parent_step_id，所有spawn/resume先Link ACK；真实mock provider首请求链通过后移除旧prompt拼接，适配原调用方。
- [x] 2.5 纯规划保留stop和tombstone、终态/重开/连续编辑不撤回终止。
- [x] 2.6 host-only固定通知/隐藏UI/安全中断/收尾ACK，保持命令与结果真实生命周期。
- [x] 2.6a ChatState保留独立PlanGuidance来源、精确durable receipt和强制AgentMessage消费，普通消息不能代替控制。
- [x] 2.6b Shell runner发送前核对source canonical change/link与真实child，拒绝伪造或未claim的规划控制。
- [x] 2.6c Shell通知投影按typed来源隐藏live/cold控制，保留普通消息及模型消费/内部证据，不按正文前缀隐藏。
- [x] 2.6d SessionActor规划控制先durable责任再typed receipt，发送有界固定正文与soft interrupt，按exact消费接纳Stopping ACK并保持真实执行/结果责任。
- [x] 2.6d.1 接纳单个控制：核本次host binding，同gate先plan后typed receipt/中断，有界固定正文与exact重试，覆盖两提交之间的持久前缀和cold恢复。
- [x] 2.6d.2 处理控制：HandleReplans与向父ACK只认exact typed receipt消费，stop记录Stopping且不撤销真实命令/结果/必需交付责任。
- [x] 2.6d.2a 修复独立review发现的投递队首阻塞：缺typed消费的历史Handled保持未确认，后续合法ACK和直属child控制仍可推进，保留原重试责任。

## 3. Acceptance

- [x] 3.1 定向单元/公共API、授权/竞态/故障恢复验证，每片与最终diff对齐。
- [x] 3.2 独立review与TODO→spawn/read→edit/remove→child收尾result的隔离真实调用链。
- [x] 3.3 同步工具/子Agent/开发说明，严格归档后全量规范与archive验证。
