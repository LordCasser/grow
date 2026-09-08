## Context
config.resolve_group_matcher 已在 Ignored 事件保留原文并返回 None；新 registry 重建曾无条件编译。事件表为唯一策略来源。

## Decisions
recompile_matcher 先检查 Ignored，清缓存并返回。其余逻辑保持；不修改通用 matcher_allows，避免破坏 Tested 缺失值时 Never 规则。

## Validation
所有 Ignored 事件经 parser、append、dedup、serde、显式 refresh，以合法和非法模式验证 matcher None 且允许 None 匹配值；已有 Tested 回归维持。
