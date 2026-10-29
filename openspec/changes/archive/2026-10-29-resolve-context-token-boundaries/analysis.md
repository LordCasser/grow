# 四项边界闭合

## 1. 低值 provider 校准
旧规则以 bytes/4 的有效请求估值作为硬下界，拒绝低值回执；这不是 tokenizer 的可靠下界。新反例 10002 本地估值/2500 完整回执在旧实现仍显示10002，零值也被忽略（见 low-anchor-before.txt）。

采用完整已接纳响应的 context count，不改变 per-attempt 计费。只保留一个滚动参考对 local/calibrated：增长加本地增量，低值 footprint 缩小按当前比例向上取整，原非负 overhead 保持。每次投影推进该参考，重复构建相同请求不会重复折算。对抗审查补充了“缩小后再次增长”的反例（regrowth-before.txt）；不能继续使用压缩前的固定比例。route/rewind 清校准，cold load 不恢复旧 native anchor。估值仍是启发式，不声称跨内容/模型等同真实 tokenizer。

## 2. 冷恢复 Goal
ChatState bootstrap 显式接收来自持久化 Behavior 与 GoalTracker 的 id/revision，仅 Goal 模式且 Active 才保留活动指令。初始化在首次 portable 估值之前完成；恢复流程若因执行条件缺失将目标 pause，在会话准入前 acknowledged 同步最终 scope。未从最后一条历史指令猜授权，也不写额外 Timeline/usage 事实。

回归矩阵覆盖 Active/Completed/Paused/Absent/Normal，首次 AvailableCommands 的 totalTokens 与 build_session_info 在任何 request build 之前一致；包含旧 revision、较新 revision 和大量被省略 reasoning，history/事件数量/消费不变。后续构建请求幂等，pause 协调重新 shadow 活动指令。

## 3. 输出预留
普通请求和 SkillLoad 不再走不同的最终窗口检查。统一采用 calibrated input + post-task-clamp explicit output <= configured total context window；删除没有路由来源的2048/256。None 保持未指定输出上限，因此本地只约束已知 input；未知后端默认输出不在保证范围，未伪造上限。自动压缩继续按已有策略尝试，最终已知总需求超窗则直接拒绝，不依赖 suppression 授权。

回归分别验证零普通 provider 派发、子任务将25000 clamp到1000后的真实 wire、精确边界和None，以及小技能已完整发布但显式输出占满总窗口时最终拒绝。技能发布时的完整分组/保留 headroom 仍为现有独立前置条件，没有被跳过。

## 4. 从实际恢复到 Pager
测试以物理 Timeline 的真实 usage 结算事实作为输入，经 MvpAgent::load_session 的 writer恢复与AdvertiseCommands捕获真正的 transient usage，再走生产 Pager ACP handler、状态行和实际 grow/session/usage 响应的报告渲染。预恢复旧快照来自该原账本，使用生产共享通知 builder 生成；测试覆盖相同总量的空 resume segment 以及新调用之后的旧低值快照，并在旧消息后重新查询服务器分段。

这验证完整链路；不以手工 JSON 代替恢复通知，也不基于不可能的同总量不同cache覆盖样本添加更多字段黑名单。只有真实反例才修改Pager排序逻辑。
