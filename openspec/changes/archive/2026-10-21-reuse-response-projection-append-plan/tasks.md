# Tasks

- [x] 添加 ignored projection 恢复基准，覆盖 128、512、2048 条合法 identity-bearing response 与全部已存在 projections，并断言无 append。
- [x] 复用 planner 产出的 canonical append plan，移除 load reconcile 路径的全量 clone、全输出构造与逐项 existing 扫描。
- [x] 增加差分及失败边界回归，验证 conflict、anchors、candidate/suppression 与 missing projection append 决策。
- [x] 串行执行验证，记录前后基准及端到端结果，完成规范校验。
