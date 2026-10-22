# Verification

固定 Rust 1.93.1、独立 HOME/GROW_HOME；首次整库测试仅 workspace-types 的 HookRegistry 样本缺字段失败。该测试函数与 b02731ea 完全一致，既有 DTO 的必填 on_failure 未被本轮改动；样本增加 allow 后 workspace-types 51/51 全部通过。生产 DTO 与缺字段拒绝逻辑未改，完整序列化相等断言保留。失败和通过日志在 evidence。fmt 与严格校验均通过。临时编译目录由正在进行的质量清理复用，完成后统一删除。
