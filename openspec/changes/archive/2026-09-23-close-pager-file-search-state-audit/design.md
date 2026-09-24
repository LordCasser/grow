## Context

`clear_context` 清除当前上下文、drill anchor 和 results。下一次从无上下文进入搜索会执行 `start_query`，重置下标、hover 和滚动偏移；`poll` 在无上下文时直接返回。实际风险在于 worker 在线程中处理 query 命令：其 tick generation 不代表请求身份，重开后仍可能读到上一查询的高 generation 结果。

## Goals / Non-Goals

**Goals:** 让文件搜索结果快照携带同步分配的 query identity，并在 Pager 应用前核验身份；提交新查询时立即清除旧结果。

**Non-Goals:** 不改生产状态语义，不处理 detached worker、队列边界或 Scrollback 搜索资源所有权。

## Decisions

`FuzzyFileMatcherDaemon::set_query` 为请求递增分配 `query_id` 并随 worker 快照返回；Pager 保存当前 id，并同时核对 query 文本和请求 id。这样同文本的新请求（例如目录 walk 模式变化）也不能接纳前一次的结果。 `start_query` 同时清除已投影快照。测试用旧请求 id 配合更大的 tick generation 验证结果被拒，再用当前 id 验证结果可用，并断言 reopen 重置交互状态。

## Risks / Trade-offs

[测试未覆盖鼠标几何] → 仅验证搜索状态和快照身份；鼠标区域有效性属于独立列表几何审计。
