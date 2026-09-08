## Why
用户指南将技能 model/effort 描述为执行覆盖，但实际字段未接入采样配置。需要修正文档承诺，并把仅存储字段的保留或删除选择交给用户。

## What Changes
记录实际调用链，修正元数据说明，将未接入字段列为 R10 删除候选。不改变采样行为。

## Capabilities
纯审计、说明及清单，skip_specs=true。

## Impact
指南、SkillInfo 注释和临时清单；不删除字段。
