# 验证记录

本次基于已有在制工作树，不回退其他任务改动。使用共享target和有限编译并发。

## 枚举增量

- tools的list_定向组42项通过，新增public list fixture最初未经过freeze准入导致Suspended断言失败；补真实FreezeAdmission与相同operation的ACK，再经实际状态投影确认后，public_list_subagent独立回归通过。没有放宽状态断言或改生产来隐藏失败。
- Shell observation_tests组5 passed：原有冷历史ACP与新public list同时通过，含真实文件、冷runtime缺失、17MiB来源事实、权限过滤、损坏Result和答案artifact缺失不阻止基本枚举。
- 该轮构建还发现旧冷历史fixture依赖进程内GROW_HOME环境切换，无法隔离已经缓存的路径。当前夹具使用独立子进程，在进程启动前设置临时GROW_HOME；针对先前残留的5个cold-observation-*专用目录核验summary的ID/cwd后已清理，不涉及原业务会话。
- Rust新增输入/输出/ToolKind都显式接入registry、taxonomy、只读权限与ACP投影；第一次编译发现的穷举遗漏已逐项补齐，没有增加未知类型宽松fallback。

## 详情、等待与command分离增量

- 公共Wait首组4 passed；tools任务相关组197 passed，包含真实coordinator batch status、runtime缺样本、completed/failed/cancelled计数、权限与Workflow过滤、分段等待、finalization真正提交后再交付，以及command只读Terminal的失败路径。
- registry组64 passed：新ToolKind与Read权限穷举，Task依赖三项subagent观察而Bash依赖command输出，实际注册/description/schema可渲染。
- 后续subagent定向组37 passed，包含公共list在资源锁耗时4秒、runtime不回复、history即时终态时仍于原五秒返回历史；没有重开预算丢掉身份。
- Shell observation_tests 5 passed：公共list/status/wait与ACP共用真实canonical冷历史、无Terminal；覆盖授权、损坏链接、未知计数与缺答案artifact。读取不启动业务child或恢复writer。
- Shell：typed wait识别1 passed、interrupted_组10 passed、completion_delivery 9 passed、notification_bridge 33 passed、compaction_context 8 passed。支持工具名与参数重命名、默认wait可中断、全部身份与真实耗时、中断不吞完成。
- Pager wait定向组82 passed，tracker全组168 passed；列表/详情不被当成等待，subagent wait无timeout仍阻塞，command仅正timeout等待，metadata/variant与旧别名拒绝均覆盖。
- Agent config定向组86 passed，包含四种preset与command/subagent职责拆分。

## 迭代中发现与修正

- command拆分后的缺导出/测试import及namespace类型在编译期逐项修正，没有保留旧TaskOutputTool兼容实现。
- 通知回归最初仍期待旧的单位置参数提示；新提示只引用已解析工具名和原ID，避免在未携带rename map时拼不可调用的JSON。启动回执仍通过真实renderer map生成可调用JSON。
- common description回归最初仍期待background_task_action承担subagent结果；已改为subagent_wait，完整common组最终结果见下。
- 移除Task与command耦合时，builder四项既有测试暴露foreground-only工具集失去构建能力。现保留原foreground归一化行为，以实际command输出+取消能力判断后台可用，而不按是否注册Task判断；显式开启但缺生命周期工具的既有归一化也保留。
- 最后一次联合编译遇到其他在制change正在写入review_binding、shell先引用而tools尚未重编的瞬时不一致；未修改或回退该change，源文件就绪后继续验证。

## 对抗审查与边界

两个luna协作者分别只读审查runtime/history权限、未知与终态合并、共享期限，以及Shell typed中断/完成收据/提示路由。未发现剩余可复现问题。主agent复查了命令与subagent同ID隔离、完成结果消费、FinalizationPending阻塞和资源队列占用期限。

这些结果来自本地测试与真实入口fixture，不等价于付费模型行为评测或运行中业务会话热升级。未重启、恢复或写入用户给出的业务会话；未执行整个workspace测试或Windows原生测试。

## 最终文件状态验证

- `CARGO_BUILD_JOBS=2 cargo test -p tools -p tool-types -p agent -p shell -p pager --lib --no-run`：五包测试目标编译通过。现有deprecated atomic与macOS debug unwind linker警告未在本次修改。
- 对该次联合构建的binary执行：tools task组198、subagent工具组4、registry组64、completion reminder组7，全通过。
- Agent builder组34、prompt组209、tool-types全组127，全通过；此前common placeholder和builder foreground失败均已闭合。
- Shell最终binary：observation组5、wait_interrupt组9、notification_bridge组33通过。permission组初次以默认测试栈运行发生debug stack overflow；按开发指南的`RUST_MIN_STACK=16777216`重跑12 passed。实际child工具等待穿过consultation的fixture另1 passed（其已有专用session栈）。这不是生产栈修复，未调整生产调用链。
- Pager最终binary：tracker全组168、resolve_turn_activity组13通过。
- `git diff --check`及全量strict OpenSpec在归档前通过；归档后校验记录如下。

最终验证后只改了说明性注释和本change记录，没有继续叠加功能。未创建隔离cargo target，复用共享target并限制build jobs=2；不删除其他任务共用的编译产物。

归档完成：`openspec archive split-subagent-observation-tools --yes`成功合入session-timeline与local-coordination。归档后`openspec validate --all --strict --no-interactive`为19 passed、0 failed；`openspec validate --archived --no-interactive`为822 passed、0 failed；`git diff --check`通过。源码中的旧混合名称仅保留在明确拒绝旧别名的测试和“禁止发明旧工具”的说明中，无实现、注册或可调用兼容入口。
