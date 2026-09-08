## Context
Pager 缓存 Shell 的 BehaviorAvailability；Agent 重建会在 ApplyingControl 期间发布命令元数据。控制 drain 返回 Idle 后必须刷新投影。Timeline 的 append_surface_items 为每次调用从 0 分配索引；Control 逐条调用造成同一事件的上下文 ID 冲突，pending 与 branch fold 也丢失索引。

## Goals / Non-Goals
修复用户的空会话切换和 resume 失败，覆盖立即与延迟控制激活。不放宽替换完整性约束，不改控制协议或增加存储实体。

## Decisions
控制 worker 正常释放 ownership 后发送当前可用性。Control 采用事件序号和原始上下文索引组成 SurfaceId，并贯穿 pending 与 branch provenance；普通消息追加逻辑保持原有语义。

## Risks / Trade-offs
延迟激活可能只保留同层最新项，必须保留原始索引，不能按激活批次重新编号。测试覆盖 step 与 turn 边界及 supersession。已有事件原文不改写，恢复按修正后的确定性投影执行。
