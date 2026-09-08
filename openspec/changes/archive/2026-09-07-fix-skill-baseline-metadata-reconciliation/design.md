## Evidence
update_startup_baseline 比较 HashSet<String> path 后替换 startup_skills，仅路径集变化时设置 pending。ToolBridge::apply_pending_skill_update 仅 take_pending 返回 Some 才更新 AvailableSkills。Shell 无 pending 时仍发布命令，slash_skills 直接读取 manager，因此不能以 UI 更新推断运行时已同步。

## Concurrency finding
list_skills_with_roots 内部没有 await；list_skills_with_plugins 的唯一扫描 await 包裹该同步完成函数。load_config 同样同步完成。当前单线程 actor 在配置快照、plugin snapshot、扫描到请求 resources 锁之间没有已确认的执行让出点；锁是 Tokio FIFO。未证实之前担心的“旧扫描晚完成覆盖新扫描”，不因此新增锁或版本字段。跨 task 提醒时序未穷举。

## Intended comparison
核对 SkillInfo 全部字段与条件技能分区后，优先使用结构化等价比较，保留顺序/优先级的语义，不序列化成 JSON 比较。条件技能激活与动态技能保留属于必须核对的相邻逻辑；不直接扩大修复范围。

## Validation plan
相同路径修改 enabled 或 description 必须产生带最新 runtime_skills 的 pending；完全相同内容仍无 pending。现有条件技能与 baseline 测试需要运行。磁盘不足，当前仅添加回归，未执行，未实施生产修复。

## Confirmed reproduction and decision
独立编译实际 SkillManager、conditional、listing、SkillInfo、ConfigSource、truncate 源文件，旧回归失败 same-path metadata change must reach runtime。仅 parser 集成入口为 panic stub，该测试明确排除，未改被测逻辑。采用 SkillInfo 派生 PartialEq/Eq，比较 take_unconditional 返回的有序 Vec 与当前 startup_skills；全部字段变化均反映到 runtime，顺序变化也不被集合掩盖。ConfigSource/SkillScope 已支持 Eq，metadata HashMap 使用逻辑键值相等，不受插入顺序影响。

## Consumer audit clarification
AvailableSkills 未找到生产读取，仅有写入和测试读；不得将内部副本陈旧表述为已证实的技能权限绕过。实际 slash 读取 SkillManager 并过滤 enabled。同路径 metadata 修复会使 BaselineChange 重新渲染技能提醒，description 更新因此到达模型可见提示。AvailableSkills 本身另列 R5，等待用户确认再删。
