# Prepare Grow 2.6.1

## Why

用户于2026-11-05要求：“停止新审查，当前作为一个里程碑，记录。然后准备收尾，以发布新版本。”本次冻结新增审查和未启动修复，整理已经接受的功能与数据一致性修复，完成仍在途的必要验收，再准备版本候选。官方GitHub Release只读核对当前latest为v2.6.0（publishedAt 2026-11-03T22:20:21Z）。

## What Changes

按用户随后明确指定“按2.6.1准备候选和发布”，准备2.6.1候选并执行官方发布：记录里程碑和剩余backlog、归档已经通过的行为change、更新workspace version/对应lock本地包版本及发行说明，并核对发布前已有门禁。不新增产品行为或改发布流程，因此skip_specs:true，不虚构delta。实际执行暴露的旧测试假设（actor终态、Minimal启动就绪）在本维护change内修正，并可增加一条不含prompt的Pager状态应用诊断以建立可观测前置；行为修复LSP与Responses分别建立并归档独立delta，不能以本维护豁免跳过契约。

## Impact

版本维护只更新workspace继承版本及其lock记录，保留独立0.1.0包与third-party版本。发行说明位于shell/changelogs/2.6.1.md；本change保留验证、源码归属、待完成发布步骤和限制。本地准备不等于官方发布；正式脚本要求clean committed tree、匹配annotated tag、精确commit的多平台构建/完整资产/attestation。用户已明确授权本次2.6.1候选和发布；必要的候选提交、annotated tag、push与官方workflow发布在该版本范围内推进，保留既有门禁且不重写已发布历史。正常Grow存储/安装/重启及真实provider/MCP/hooks不触碰。
