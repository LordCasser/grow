## Why

用户要求完成本轮修复、文档与任务收尾，再递增最小版本并正式发行。v2.6.15和v2.6.16均被官方发布门禁阻止，tag已推送并保留，正式最新版仍为v2.6.14。远端核对v2.6.17未占用，本轮使用该后继patch。

## What Changes

- 纳入已验收的会话恢复展示、Plan权限/提示、连续历史、Turn用量与gap锚点，以及独立维护的Windows测试cfg、无调用入口清理和分页Tasks恢复。
- 同步workspace及锁文件33个本地继承包的patch，保持外部和独立版本依赖不变；更新准确的发行说明。
- 提交干净候选、建立新的annotated tag并通过静态preflight；推送main/tag，从默认分支运行全部平台、publish=true的官方流程。
- 核对同一SHA的Core、三平台原生协调、Windows存储、OpenSpec与十平台分发构建；公开后验证资产清单、checksum、attestation及本机下载binary，再归档并保持main同步。

## Scope

skip_specs：本change仅负责版本、说明和发行维护，不引入产品行为。行为契约由各自功能change承载；旧候选tag不修改，不以部分平台成功替代完整门禁。明确暂缓的其他backlog保持其原停止点。
