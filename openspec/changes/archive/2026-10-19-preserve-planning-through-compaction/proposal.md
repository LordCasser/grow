# 压缩期间保留当前计划投影

## Why
Pager 根 session 收到同步 AutoCompactCompleted 时会清空 `AgentView.todo`。该展示状态由最近的显式 Plan 更新建立，而当前局部计划事实由 Timeline 持有；压缩完成并不代表计划完成或被替换。child 分支也需保持一致的投影语义。

## What Changes
同步或异步 compaction 完成只更新压缩状态，不清空当前 TODO 展示；新的显式 Planning/Plan 更新仍可按原路径替换投影。

## Non-goals
不改变 TODO/Timeline 的持久化 owner、压缩上下文组装或 TODO 完成语义。
