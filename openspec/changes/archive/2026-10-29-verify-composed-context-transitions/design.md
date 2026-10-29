## 组合不变量
- 低值校准每次已应用投影推进同一参考；多次合法压缩、追加与重复 build 不丢失保留内容，不影响累计消费。
- 冷恢复的 Goal 作用域由当前恢复授权决定。切 route 清校准但保留该作用域；用户rewind在Goal仍存在时按既有规则拒绝且不改状态；内部durable历史重放独立验证不从旧tag激活旧Goal；失败 durable rewind 不改变 scope/pressure。
- 自动预压缩判断与最终准入使用同一个输入+实际显式输出预算。trigger 保留输出预留以通过StepEnded边界、图片重投影和prune复核；不能用只看输入的gate提前宣称收敛。
- 一次压缩或后台 promotion 完成后重新检查总需求；仍超窗返回错误，已提交 replacement 仍Completed，不连续重发摘要。真正 route/Agent 改变时既有机制丢弃旧 projection trigger。

## 方法
先运行新组合反例并记录red，再修改调用链。用真实ChatState/SessionActor与脚本化provider，区分summary与普通请求，覆盖成功、仍不够、抑制/子任务、None/精确边界、pre-prune与promotion相邻路径。测试注册沿现有模块，避免复制harness。

## Goal rewind入口边界
SessionActor::handle_rewind在任意Goal状态仍存在时拒绝，要求显式clear。公开组合验证必须遵守该产品约束，不能绕过入口把内部ChatState rewind当成用户可达操作。内部durable rewind用于机制测试，另标证据范围。

## 已确认的 Goal clear 组合反例
恢复后 clear 的 Control 已 durable 且运行时已清除，但 ChatState 仍持有上次请求的 Goal scope；下一次 build 前计数与当前授权不符。仅在 clear 成功修改运行时后、发布命令状态前使用既有 acknowledged scope setter 同步 None。失败持久化分支不改 scope；setter 失败直接返回错误，不回滚已落盘的 clear。其余 Goal 生命周期同步作为独立审计项记录，不扩展本次修复。
