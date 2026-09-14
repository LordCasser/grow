## Context

`usage_from_timeline` 当前把 decode 错误转为 None，并通过 contains_key 跳过所有重复身份。`spawn_from_validated_timeline` 在 launch 前调用 infallible `ChatState::from_timeline`，因此异常账单也能发布。

## Decisions

1. 令用量 fold 和 from_timeline 返回现有 TimelineWriteError；新建内存 seed 不含用量 Observation，维持 infallible new 的内部断言。
2. 保留现有 typed settlement。由 settlement 自身比较同身份载荷，实时与恢复调用相同判定；不新建事件注册表或第二套账本。
3. 用量已知事件 decode 错误包含 event seq、scope/name 和具体原因。incomplete/resume 标记只接受无 data 的既有格式；未知 scope/name 不参与用量 fold。
4. fold 失败发生在 actor launch 前。此时中断恢复仅修改独占的内存 Timeline，尚未提交任何 recovery event，原持久化文件不变。
5. 保留现有正常 writer 的字段语义；本项不另加空 model、预算范围等未经确认的产品约束。

## Validation

真实 actor 恢复覆盖 attempt/child 合法重复、冲突载荷、缺失/类型错误/未知字段、两个 marker 的异常载荷、未知诊断 Observation；失败时确认无 actor 更新事件和无 persistence record。已有 lifetime/resume 分段与实时幂等测试继续通过。
