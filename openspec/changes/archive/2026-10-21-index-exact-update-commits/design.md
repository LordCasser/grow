## Context

基线 HEAD a1d1c2965075c87645eab22adce6d48b19410c59。raw_acp_event_status/raw_projection_status 每次全读 updates；同步成功后的物理可见性与失败重试已有专门契约。

## Decisions

- 索引归 JsonlStorageAdapter 的 writer epoch，按 session key 共享到 clone；只保存身份与物理位置，不复制全部正文。
- file identity/stamp 不变才复用；普通 owner append 维护已知新增记录，未知变化、截断或替换完整重建。
- 相同 key 的全部物理记录均须 exact，一条冲突即 Conflict；不把 index hit 当 durable ACK。
- 沿用现有 lenient update decode 和 torn-line 处理，读取边界保持现有目录能力。

## Validation

先用相同 ignored fixture 测旧 raw lookup 与实际 commit，再测新实现。另测普通/exact 混排、重复/冲突、同长度外部替换、文件替换、失败 sync、重开与尾部残片。只使用临时数据。
