## Why

用户报告 session `01a19a05-5281-7201-bae3-89f599512353` 无法 resume，随后明确要求先修复本机 session 文件以恢复可用。

## What Changes

- 在隔离副本中核对 checkpoint 的 runtime source hash，证明差异仅来自项目 AGENTS.md。
- 持有原会话 writer/append locks，备份后仅重新绑定最新 checkpoint 的 source hash，并重算该条物理记录校验和。
- 保留原 session、Turn、树挂起身份、历史、续跑位置和所有其他记录；移除临时诊断代码。

## Capabilities

不改产品契约。`skip_specs: true`：本 change 记录一次用户授权的本机数据修复，不改变正常恢复、来源验证或存储格式，不虚构 delta。

## Impact

仅目标 session 最新 checkpoint 和本 change 记录；不安装二进制、不放行原任务执行、不修改项目 AGENTS.md 或 Agent 定义。
