## ADDED Requirements

### Requirement: Refreshed baseline supersedes same-path discoveries
新 baseline SHALL 接管其包含的规范技能路径，旧动态副本 SHALL 不遮蔽该路径的新元数据或条件门控；未包含的动态技能 SHALL 保留。

#### Scenario: 同路径动态副本被刷新
- **WHEN** 已动态发现的技能进入更新后的 baseline
- **THEN** 投影使用最新 baseline 内容，其他路径动态发现不丢失。

#### Scenario: 新增条件门控
- **WHEN** 先前无条件动态技能在新 baseline 中具有尚未触发的 paths 条件
- **THEN** 旧动态副本不使它继续可见。
