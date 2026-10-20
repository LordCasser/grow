## Why
用户要求将子 Agent 权限 Settings 入口与四项测试清单补齐，在回归通过后提交、推送并发布新版本。

## What Changes
发布补丁版本 v2.4.3；更新 workspace 和 lock 的本地包版本及发行说明。将已完成的行为 change 和测试/配置入口审计一并提交，通过正式十平台构建及同 SHA 回归后发布。

## Boundary
纯版本、说明与发布维护，skip_specs；菜单行为由 expose-subagent-permission-setting 归档负责，测试维护与只读审计由 complete-settings-preference-test-inventory 负责。不修改已发布 tag，不合并其他分支。
