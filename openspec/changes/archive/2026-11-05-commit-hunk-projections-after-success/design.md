# Design

父核对actions.rs::apply_hunk_action在fallible accept/reject前提交stats/turn_index；apply_action_batch也在每file I/O前提交每hunk统计、turn索引和affected ids。reject_hunk/reject_hunks_batch已在I/O之后才修改file state/events，职责合理；上层过早投影是同一owner的一处规则错误。

## Decision

公开handle/action错误类型签名保持，单项成功后再提交该hunk统计和移除旧turn引用；文件批动作成功后再提交本文件全部统计/turn索引/affected ids。保持现有单文件batch patch/recompute顺序，事件已经由底层动作发布，不新建事务/事件总线。跨file批次是逐文件提交：后面文件失败返回既有Err，已成功文件保持既成结果，失败文件不计成功；不能回滚磁盘或清空所有投影假装atomicall。

这是后续stale磁盘拒绝的前置，06a将使用同一Err失败通路；本轮不实现stale检查、Conflict类型、三方合并或watcher改造。无新公共实体。

## Coder Task Spec

单一修复：成功之后才提交动作统计/turn索引。write仅hunk-tracker/src/actor/actions.rs和同crate必要tests（公共handle真实入口，不只私有helper）。保留normalAccept/Reject、单/file/turn/all的正确hunk数和内容；真实写/删失败时原file hunk、turn_hunks、session_stats均未提交且无成功移除事件。批次有先成功文件后失败文件时，断言失败文件仍pending，统计只含已成功文件；若HashMap顺序无保证可通过稳定夹具或观测已成功项，不为测试改产品顺序。

Tier2公共Handle验证+独立luna review提交边界，父在06a集成再做staleErr同投影不变量。错误注入用/tmp目录占位或权限/owned测试I/O，无真实用户文件，不删失败场景。Rust/Cargo/rustdoc1.93.1，MINSTACK16MiB，独立GROW_HOME。父独占编译窗口调度：先源码候选+rustfmt/diffcheck，冻结通知再跑Cargo。父拥有OpenSpec/docs/backlog，coder不混修其他audit项目。代码和公共tests满足即可验收本修复，完整数据里程碑另行验证。
