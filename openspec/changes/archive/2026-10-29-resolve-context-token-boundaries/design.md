## 单一事实与转换
- 校准以最近一次完整 provider count 和其对应本地有效估值为一份参考。高于估值时保持现有非负 overhead 语义；低于估值时新追加内容按本地增量计入，缩小现有 footprint 按当前校准参考比例估计，避免负 offset 在压缩后饱和丢失内容。精确锚点与重复 build 保持不变，route/rewind 清校准，usage 不受影响。
- Goal scope 由 restored Behavior=Goal 且 Goal=Active 的 id/revision 决定。bootstrap 参数初始化 ChatState 后才启动 actor；运行时不可恢复导致 pause 时，在暴露会话前用同一作用域入口同步，不猜历史 tag。
- 最终请求准入采用 calibrated input + request.max_output_tokens.unwrap_or(0) <= window。max_output_tokens 取子任务 clamp 后的实际值，技能请求共用；未知后端默认输出明确不在本地保证内。
- UI 以实际恢复账本/AdvertiseCommands 快照作为测试输入，经生产 ACP handler 与状态投影检查；旧快照必须不能回退已恢复的消费或 segment 信息。排序修复仅在反例确认后按既有权威源实现。

## 验证
先写触发旧规则的回归，记录 red 输出；修复后执行 ChatState、Shell 准入/冷恢复、Pager usage/reconnect 定向与适当全量回归。不能把独立组件测试当成端到端闭合。来源缺失、权限/Goal 变化、零/低值/极大值和精确窗口边界分别核对。所有构建结束后清理本次 Cargo 产物。

## 对抗审查补充
最初静态参考比例在“先缩小再追加但仍低于原 reference”时，会把新增内容继续按比例低估（1000/250 → 400/100 → 500/125，应为500/200）。补回归后改为每次已应用投影推进同一参考对；相同投影幂等，不引入第二份基线。正 provider overhead 的原语义保持。连续向上取整缩小可能保守多几个 token，不将启发式结果当模型 tokenizer 的精确计数。

Cold bootstrap 直接在 ChatState::from_timeline 的首次估值前设置 scope，避免长历史重复扫描。持久化控制状态的修复仍先 durable；scope 本身是请求投影参数，不另写历史或用量。
