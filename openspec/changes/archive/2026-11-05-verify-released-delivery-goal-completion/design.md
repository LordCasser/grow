## Contract and isolation

使用TestSandbox的独立GROW_HOME、git workspace和env_clear子进程，启动本次target/debug/grow；parent/child mock provider分开endpoint，避免错误消费脚本。所有控制都针对该私有子进程。正常会话与~/.local/bin/grow完全不动。

实际链路：native create_goal → todo_write建父step → spawn_subagent(background,required_deliverable,parent_step_id) → 从真实Task结果取得child id → todo_write Released(None) → 更新父step产生pending replan → native update_goal Complete必须被真实completion blocker拒绝 → native kill该自建child并wait实际终态 → canonical结果通过PlanningTaskResult提交、pending replan结算 → native update_goal Complete成功并落盘completion_report。

用一个持续阻塞provider的自建child制造明确live execution，取消/等待是本测试自身的合法处置；不伪造dispatch_failure、ResultRef或terminal，不直接写Planning快照掩盖入口。若测试组织需要动态tool参数，使用测试本地HTTP handler读取前一请求的真实Task结果；不修改通用生产/测试框架。可复用现有Goal command启动，但终结必须经过native update_goal。

## Acceptance

断言提前release不终止child或清pending，首次Complete返回具体未处置执行/更新阻塞；取消后canonical parent Spawn/Ended、child Seed/Result exact linkage真实存在，父link terminal/ref与该result一致；历史Released None原样、无Accepted伪造，pending replan变Unapplied/清空；最终Goal Complete与报告在同一Control持久化。冷Timeline重读仍通过并保留最终状态。实际选择并运行此ignored测试，使用当前binary和独立GROW_HOME，进程自行关闭后无自建writer遗留。

## Review and boundary

一个coder只负责这个集成回归；父核对fixture隔离、实际native入口和结果，运行/必要修复后归档。只证明隔离新构建的链路，不把它当当前正常会话中的旧子任务已恢复；该现场仍按用户指示保持运行。
