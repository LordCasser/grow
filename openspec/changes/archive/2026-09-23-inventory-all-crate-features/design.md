## Decision

`crate-inventory.json`、`feature-map.json` 和 `reviews/` 保留原始审计资料及其来源哈希。`reviews/draft-deltas/` 保存未合入的候选规范，目录名称明确其非权威地位。主规范仍只以 `openspec/specs/` 为准。本 change 设置 `skip_specs: true`，归档时不合入候选 delta。

## Evidence boundary

61 个 manifest 和 2,163 个登记源码文件的快照身份已核对；包级 `reviewed` 标记只说明该快照有审阅资料。功能映射与来源缺口详见 verification。原 Pager 深度覆盖及跨 capability 语义统一没有完成，也不被重新解释为完成。任何复用草稿要求的后续 change 都须重新核对当前实现、调用方、失败场景和测试。

## Trade-off

停止继续生成全仓细粒度要求，避免规范膨胀和错误权威化；已有大规模扫描成本作为可追溯资料保留。后续修正规范的粒度由具体行为边界决定，而不是 crate 或文件数量。
