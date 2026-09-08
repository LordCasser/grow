## Why
自由文本审计发现 alternate ID formatter 在选择有效 ID 时丢弃 notes；需先确认真实入口，避免维护未接入分支。

## What Changes
记录真实调用证据并新增临时删除候选 R4，等待用户确认，不改产品行为。

## Impact
本 change 审计记录与用户明确要求的根目录临时候选文件；无契约改变，skip_specs。
