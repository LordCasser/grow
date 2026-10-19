## Context

实施开始 HEAD 为 `8cc20454`，workspace 版本 2.3.1。原有未提交代码和 change 已记录工作区 manifest，改动必须保留。新版本以 2.4.0 为目标，最终通过的能力与限制写入 release notes，不能把静态规范通过算成实现通过。

## Decisions

| 顺序 | 工作 | 共享约束与验证门槛 |
| --- | --- | --- |
| 1 | 窄采样修复、现有 SDK 接点核查、Plan canonical 读取 | 保留原有 null usage 修改；不打开缺乏宿主控制的 MCP 能力 |
| 2 | 统一 Timeline 的计划/运行生命周期事实 | SessionActor 仍是 owner；schema 只按实际落地类型统一更新；不建立平行 store |
| 3 | 计划命令、稳定 task 关联、结果直接父级交付、重规划与真实等待 | 与 Goal owner、root lifecycle owner 分离；单次逻辑交付、不重置累计限制 |
| 4 | 树级挂起与同身份恢复 | 整树准入栅栏、原 turn/child/操作位置、五秒总期限、writer drain、中断未知不盲重放 |
| 5 | 保留 Plan 与统一 Skill 加载 | Plan 候选独立于阶段但复用 Control；Skill 原输入及整组结果同提交，恢复不重读或重发 |
| 5a | 修复会话绑定时的输入快照覆盖 | 联测发现已有 CWD 草稿会覆盖刚出队的首条输入；由 `preserve-input-during-session-bind` 最小 change 保留原 checkpoint 门槛并验证真实 worker/PTY |
| 6 | MCP 本版暂缓 | 用户明确保留 SDK 门禁；不实现或归档未满足接点的交互 |
| 7 | 全部功能联合验证与版本准备 | 联测等待中的 Skill/Plan 和冻结、Goal 完成、权限及用户输入优先级；验证后逐项归档 |

并行工作只用于边界清楚的确定性子任务，构建与测试集中串行执行。对同一 Timeline/Control/runner 的修改由主实施路径整合，避免多方覆盖。按受影响 crate 定向测试，再做集成与真实进程验证；监控并清理本次 Cargo 产物。

## Cross-change Invariants

1. 冻结只关闭运行准入，不改变 Goal/Plan/Workflow 的业务状态；现有用户/权限等待不得被自动通知批准。
2. TODO、Skill 上下文、Plan 候选及 MCP 事实分别有类型化来源，均复用 Timeline/Control；历史副本不携带执行资格。
3. 已接纳副作用与结果不因 resume、重规划、模型切换或 MRTR 而重放；未知状态先核对。
4. Skill 准备完成不等于组发布；Plan 保留不等于批准；URL 同意不等于外部操作完成；消息消费不等于重规划处理。
5. 子 Agent 的直接委派者、执行 owner 与 root 生命周期 owner 不混用；局部计划不越权修改 root Goal。

## Verification

每个 change 保留实际验证记录与未执行项。联合场景特别覆盖：child 等待中的冻结、Consumed 未 Handled 的变更、待验收结果、Skill 准备/发布前后、保留或待批 Plan，以及无跨进程续接保证的 MCP 在途调用。只有完整实现并通过场景的 change 才归档；官方 SDK 或真实服务缺口必须记录，不能用自写协议或 mock 成功替代互操作结论。

Workspace 的 Downloads 分类测试不能假定开发机器的实际目录没有 Git 仓库。测试应按既有“Git 项目优先于普通系统目录排除”契约判断当前目录；本次只修正夹具断言，不改变分类行为，也不修改用户目录。
