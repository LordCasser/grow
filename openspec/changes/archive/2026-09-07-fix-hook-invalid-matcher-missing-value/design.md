# Design
matcher_allows 先识别内部 Never 变体并返回 false，再执行原有 matcher/value 分支。恢复仍由 recompile_matchers 将非法正则设为 Never，无需在 dispatcher 重复编译。回归覆盖缺失字段、空字段和正常字段，以及有效 matcher 缺失字段仍允许。
