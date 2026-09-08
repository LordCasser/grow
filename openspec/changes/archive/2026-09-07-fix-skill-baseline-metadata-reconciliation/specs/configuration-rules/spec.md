## ADDED Requirements

### Requirement: Skill baseline metadata updates reach runtime
技能 baseline 重读 SHALL 将同一路径的元数据变化协调到运行时技能表；完全相同的 baseline SHALL 不因重复加载产生多余协调。

#### Scenario: 原路径停用或修改描述
- **WHEN** baseline 路径不变但 enabled 或 description 改变
- **THEN** 产生协调结果，runtime_skills 携带最新元数据。

#### Scenario: 完全相同的重读
- **WHEN** baseline 内容与顺序均未改变
- **THEN** 不产生新的 pending baseline 协调。

#### Scenario: 描述更新进入提示
- **WHEN** 可见技能在同一路径更新描述
- **THEN** baseline 协调产生的技能提示包含新描述；停用的技能不出现在新提示中。
