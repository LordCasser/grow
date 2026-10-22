# Design

先完成并测量 R5，再实施本项。writer load 的 canonical 构造与 append 决策共享，结果随同已校验 Timeline 移入同次恢复上下文；不得将不同快照或会话的派生结果组合。若 replay 前 Timeline 发生变更则弃用该结果重新构造。只读 resident load 保留现有自包含构造和 frontier/delta 语义。

raw replay 仍在其固定 updates 上界内逐条解码 projection 并 exact_match；planner 将第一条 exact physical row 放到 candidate anchor 或原 exact 位置，无 exact 时才编码新的 canonical 正文。changed 必须继续表示抑制/重排，包括删除重复 exact，因 cursor 只在 unchanged 时可按物理位置增量处理。canonical_appends 必须独立记录无 exact 的 projection，避免借用优化改变 durable repair 决策。

验证正常、缺失修复、重复 exact、异正文冲突、rewind分支、cursor回退、resident竞争及迟到 delta、fork继承和路由变更回退。沿用与R5相同的现代基准，比较R5后→R6后，并测真实PTY首屏、ready、历史推进及Ctrl+D。实现前进一步核对数据所有权和全部调用方，保持最小结构。

## Borrowing and provenance

补充周边核对发现 transcript 用 ReconciledReplayLine::Owned 判定 simulated_source；把重排后的 exact 直接标成普通 Borrowed 会改变该语义。保持 planner 的 Canonical 决策，附带已验证 exact 的物理位置；raw 结果将既有 Owned(String) 表达改为 Synthesized(Cow<str>)，missing时Owned、exact时Borrowed，但两者都保留原合成来源标记。普通物理 Existing 仍 Borrowed，不改变 cursor unchanged 推断和 transcript anchor/Timeline 时间优先级。该最小表达调整直接服务于保持现有契约，非通用缓存框架。
