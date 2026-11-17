## Why

用户要求本轮修复、文档和任务完成后发布最小patch，并进一步明确“确保tag发布递增，中间出的问题就撤销tag”。正式最新版为v2.6.14；三个未公开候选v2.6.15/16/17均经release404及对象核对后按用户授权撤销tag，提交和失败run保留。本次修复后重新发行v2.6.15，保证公开版本连续递增。

## What Changes

- 纳入已归档的恢复展示、Plan调研、连续历史、Turn用量、gap、Tasks状态和resident Behavior快照修复。
- workspace及lock中33个本地继承包从候选metadata2.6.17改为2.6.15，不动外部及独立版本；重写最终发行说明。
- 新提交建立annotated v2.6.15，通过clean-tree preflight后原子推送main/tag，默认分支执行十平台和全部必需门禁。
- 公开前完整资产/attestation/checksum由正式workflow验证；公开后独立核对release、11项资产、下载native包签名/version/help及来源SHA。

## Scope

skip_specs：仅版本、发行及记录维护，产品行为由归档契约负责。失败tag先撤销再修复重试同patch；已经公开的正式release和tag不得按失败候选处理。未授权的其他backlog保持原范围。
