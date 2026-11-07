# Analyze residual sampling decode failures

## Why

用户在LSP/Responses修复及2.6.1发布后询问是否还会出现同类问题。本次只分析相邻采样解码、服务错误传播和工具声明运行版本边界；不能把已处理的三个现场形状当作所有路由均已兼容的证明。

## What Changes

核对当前8ab486a6源码、归档model-sampling契约、锁定SDK和原有回归，用隔离合成输入区分当前实现缺陷、受限兼容范围和缺少真实路由证据的候选。结论与复核证据放本change，有证据的延期项登记backlog。skip_specs:true：本次无业务代码、接口、状态、持久化或行为契约变化，不虚构delta。

## Scope

仅Sampler client/SDK事件形状、共享流错误分类与现L2接纳影响。实验不连接真实provider，不读取无关用户会话，不改公开2.6.1/tag/资产或安装/运行进程，不开展全仓审计，不派发实现修复。临时探针只在隔离worktree或/tmp；主工作区仅写分析文档。历史修复仍有效，是否后续实施应按各独立所有者和证据另行立项。
