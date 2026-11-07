# Avoid replaying validated history during exit

## Why

用户报告长上下文双Ctrl+D基本必触发五秒超时。父与只读调查已核共享deadline在MvpAgent::start_exit_suspension，Pager也有五秒RPC观察。退出的runtime_snapshot至少三次经ChatState GetTimelineEvents深拷贝全部事件，再CanonicalChildFacts::read→Timeline::from_events完整校验/重建，只为取得Spawned/Ended。Session suspend_node另有全量events/surface副本。这是已确认随历史增长的机制；具体现场根因和耗时比例仍需可比实验，不能凭静态事实声称已复现。

## What Changes

先在ChatState canonical owner提供一次性窄runtime-tree查询，再由Shell挂起消费已校验事实；cold来源仍沿原完整验证。该增量消除对无关历史正文的复制/重复重放，不延长共享五秒deadline、不弱化持久确认/树完整性/writer退役。用长历史真实入口及阶段测量判断是否仍有范围内瓶颈；只有新证据支持时才增加下一最小增量。

## Impact

ChatState查询接口与Shell runtime_handoff consumer、定向回归和长历史产品链。无持久schema变化、无新cache/registry、无默认跳过验证或强制退出。
