# 修复后 v2.6.15 发布验收

## 版本规则与失败tag撤销

用户明确要求“确保tag发布递增，中间出的问题就撤销tag”。正式latest API为v2.6.14，三个失败候选均无release（逐一HTTP404）。核对本地远端tag对象完全一致后，使用精确对象force-with-lease原子删除v2.6.15/16/17远端引用，再删除本地；远端复核为空，正式v2.6.14未动，main提交历史保留。

旧对象及失败证据见[最后失败候选](../2026-11-17-prepare-release-v2-6-17/verification.md)。新发行只按公开版v2.6.14→v2.6.15递增；若本次未公开候选仍失败，同样先撤销tag再修复重试，不消耗新patch。

## 产品验收

恢复展示、分页Tasks及gap/Windows资格/死路径收尾均已独立验收归档；完整Shell/Pager12612项、本地官方38PTY全部成功，v2.6.17同产品SHA官方Core、Windowsstorage和两个Windows分发构建亦通过。最后[resident Behavior修复](../2026-11-17-fix-resident-behavior-snapshot/verification.md)具备确定性heldACK旧红、新Control/caller13项绿，以及原真实跨进程脚本10类绿；新默认CLI、release-dist-Dwarnings、workspace/all-targets全部通过。未修改原失败脚本断言。

行为验证时metadata为2.6.16/17，当前只将33个本地继承包改回2.6.15并同步说明。父用locked metadata和完整lock文本比较证实：除这33个version行外Cargo.lock与HEAD字节完全一致；外部和独立版本均未变化。1.93.1 metadata、全仓fmt、diff空白通过；18份Markdown436处相对链接/requirement锚点通过，OpenSpec all strict19项及archive校验通过。历史未发布说明改用原commit链接，撤销tag不产生失效说明链接。

## 正式候选与运行

- 候选commit `dabf56752437a29cd1429803d47af9d802409300`，标题Release Grow v2.6.15 with verified resume and resident snapshots；annotated tag object `668fb600f4b4efb960b32759213d10c47e8d2d16`。固定Rust1.93.1 clean-tree `bash scripts/validate-release.sh v2.6.15`退出0。
- 推送前remote main复核为0286920e96f9d4c3e09d2aaad594aacceb7786a3，v2.6.15引用为空；HTTP/1.1原子推送main/tag后，远端main和tag解引用均为dabf5675，v2.6.16/v2.6.17引用仍不存在。
- 从main开启[正式run43993227806](https://github.com/LordCasser/grow/actions/runs/43993227806)，全部四平台组和publish=true，headSha核对与候选一致。全部必需门禁在正式流程内保留；push触发的重复协调43993227680、storage43993227643、Core43993227927已取消以释放runner，不作为通过证据；OpenSpec自动运行保留。
- Core job133739073113已成功；最终tag的日志中16个无过滤测试组共19017 passed/0 failed、40个既有ignored（包括Shell4878/Pager7737），其余精确选组和已登记PTY步骤全部成功。
- 正式run43993227806最终completed/success，20个job全部成功：十平台构建、三系统原生协调/导出、Core、Windows storage、OpenSpec及发布步骤均通过，没有跳过必需门禁。

## 公开与资产复验

- [公开release](https://github.com/LordCasser/grow/releases/tag/v2.6.15)于2026-11-17T02:21:36Z发布，API为draft=false/prerelease=false，latest=v2.6.15；版本按最后正式版v2.6.14连续递增。
- 精确11项资产：Linux x86_64/aarch64/riscv64及两种musl、macOS x86_64/aarch64、Windows x86_64/aarch64、OHOS aarch64，共十个tar.gz，加SHA256SUMS。正式publish job133768778229下载全部远端包，对十包逐项验证build-one.yml attestation，生成再下载checksum清单复核；日志中十包两轮均为OK，之后才公开。
- 发布后独立脚本task01a1e752-acc7-74b1-951d-2046bd6dd036 exit0（30.21s）：再次核对固定dabf5675的所有job全成功、公开非prerelease、精确11资产、完整有序十包清单及每个GitHub资产digest均相等。
- 独立下载macOS aarch64包，SHA256=`19aa6f15ad12dc39c973e6088fec99eb9f03fb48f62f2cbe78f83c247d1fb2f1`，与清单/API一致；包内唯一regular grow可执行文件、codesign --verify --strict通过。隔离GROW_HOME与cwd实执行返回`grow 2.6.15 (dabf567)`，--help正常；gh attestation verify绑定build-one.yml与完整source-digest=dabf56752437a29cd1429803d47af9d802409300通过。
- 发布后远端再次核对：正式v2.6.14保持原733d2c72，v2.6.15对象668fb600→dabf5675，失败的v2.6.16/17引用为空。完整失败/撤销过程保留原commit和workflow，不混为正式发行。

## 最终文档

开发指南补充用户要求的tag规则：未公开失败候选先核对再撤销tag、修复后重用同patch；公开正式tag保持稳定。公开验收及指南已以9912b11b提交并推送main，提交后工作树为空、local HEAD与remote main一致；此前格式空白、433处文档链接/锚点、当前19项严格规范及全部归档校验通过。本轮产品与发行验收已闭合，归档由后续独立文档提交保存，不移动已公开tag。归档后当前18项严格规范、全部archive校验、移动后的相对链接和git diff --check再次通过。
