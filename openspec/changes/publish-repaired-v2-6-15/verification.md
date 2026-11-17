# 修复后 v2.6.15 发布验收

## 版本规则与失败tag撤销

用户明确要求“确保tag发布递增，中间出的问题就撤销tag”。正式latest API为v2.6.14，三个失败候选均无release（逐一HTTP404）。核对本地远端tag对象完全一致后，使用精确对象force-with-lease原子删除v2.6.15/16/17远端引用，再删除本地；远端复核为空，正式v2.6.14未动，main提交历史保留。

旧对象及失败证据见[最后失败候选](../archive/2026-11-17-prepare-release-v2-6-17/verification.md)。新发行只按公开版v2.6.14→v2.6.15递增；若本次未公开候选仍失败，同样先撤销tag再修复重试，不消耗新patch。

## 产品验收

恢复展示、分页Tasks及gap/Windows资格/死路径收尾均已独立验收归档；完整Shell/Pager12612项、本地官方38PTY全部成功，v2.6.17同产品SHA官方Core、Windowsstorage和两个Windows分发构建亦通过。最后[resident Behavior修复](../archive/2026-11-17-fix-resident-behavior-snapshot/verification.md)具备确定性heldACK旧红、新Control/caller13项绿，以及原真实跨进程脚本10类绿；新默认CLI、release-dist-Dwarnings、workspace/all-targets全部通过。未修改原失败脚本断言。

行为验证时metadata为2.6.16/17，当前只将33个本地继承包改回2.6.15并同步说明。父用locked metadata和完整lock文本比较证实：除这33个version行外Cargo.lock与HEAD字节完全一致；外部和独立版本均未变化。1.93.1 metadata、全仓fmt、diff空白通过；18份Markdown436处相对链接/requirement锚点通过，OpenSpec all strict19项及archive校验通过。历史未发布说明改用原commit链接，撤销tag不产生失效说明链接。

这些结果不替代最终新SHA的正式全平台门禁。新候选preflight、正式run与公开资产结果随后追加。
