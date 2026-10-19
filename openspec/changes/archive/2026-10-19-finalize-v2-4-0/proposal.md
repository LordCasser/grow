## Why
用户要求本次全部改动提交合入 main、删除本任务 codex 分支，并进行发布前文档一致性核对。用户随后撤销 2.3.2 改号，最终保持 2.4.0。

## What Changes
保持 workspace/lockfile/发布说明版本 2.4.0；补齐当前发布说明中图片恢复与空问询 ID 修复，核对现行用户指南、开发文档、主规范和归档引用。此前被排除的四项已归档 change 及实现本次全部纳入。

本 change 不改变产品行为，skip_specs: true，不新增 delta；各项行为由已归档规范定义。MCP 多轮、Remote Control、Provider/Recap 方向的暂缓决定保持有效。验证后提交所有改动、合入本地 main，并仅删除 codex/v2-4-0，不删除其他 codex 分支。本轮进行发布前核对，不创建 tag、不触发发布或安装。

## Impact
发布说明、现行文档、OpenSpec 完成记录及本地 Git 分支。保留既有阶段验证和范围决定，不把过去未纳入的记录改写为已提交；在新记录中说明本轮全部纳入。真实 provider、跨平台发布构建与 tag 预检不由文档一致性核对替代。
