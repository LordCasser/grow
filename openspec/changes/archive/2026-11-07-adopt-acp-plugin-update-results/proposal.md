# Adopt ACP plugin update results

## Why

AC-07 的实际入口`grow/plugins/action`经SessionActor处理Update；该分支直接调用Git updater，却丢弃其new_commit/plugins，最后保存旧InstallRegistry。checkout更新后安装记录仍停在旧版本，下一次同tip更新也可能重复报告changed。CLI现有`plugin::update_plugins_by_selector`已经采用结果并保marketplace事务来源，无需另写一套采用逻辑。

## What changes

SessionActor Update调用现共享application入口，只负责把既有RepoUpdateOutcome映射为ActionOutcome和reload提示。成功的branch更新后，checkout、安装commit和发现的plugins一致；再次同tip更新为no-op。维持现plugin_id最后一段选择、空目录NotFound、pinned/local及changed才reload规则，不改变wire schema或立即重建其它session。

## Impact

一项SessionActor Update行为，复用Shell已有安装状态owner。安装、卸载与save/removal失败的AC-08逐操作另交；该项不把共享函数的warn-and-continue算持久化错误已修，不预建通用事务框架。开发说明及backlog仅在真实ACP＋本地Git验证后同步完成状态。
