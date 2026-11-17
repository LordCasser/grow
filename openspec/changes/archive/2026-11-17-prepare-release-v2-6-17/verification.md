# v2.6.17 发布验收

## 发行身份与前置事实

- 正式最新版API返回v2.6.14，published_at=2026-11-15T17:57:30Z，非draft/非prerelease。
- remote main仍为bb9b4c995794db318288beddb00a2cad27e16e70；v2.6.17远端tag查询为空。本轮尚未创建候选tag。
- v2.6.16官方run43925904022最终failure，publish skipped，release API404。Windows两包dead-code、Core Tasks恢复和macOS runner容量原因分别留证于[失败候选](../2026-11-17-prepare-release-v2-6-16/verification.md)。v2.6.15同样为未公开失败候选，旧tag均保留。

## 当前验证状态

分页Tasks恢复的索引、Shell和Pager模块均已亲读验收，并已[归档](../2026-11-17-fix-paged-task-history/verification.md)。固定Rust1.93.1验证：默认CLI构建及原失败Tasks冷恢复PTY通过；Shell4877/Pager7735共12612项库回归通过，29项既有ignored；完整官方38项PTY按target/name/顺序比对一致且每项exact通过；正式release-dist -Dwarnings与workspace/all-targets通过。格式、相对链接/requirement锚点、OpenSpec严格及归档校验通过。

这些行为验证发生在最终产品源码、版本递进前（metadata=2.6.16）。后继2.6.17只同步本地版本及说明，不重复产品实现；正式同SHA全平台/公开资产仍待本记录追加，不把本地通过等同正式发行完成。

## 版本与候选静态检查

- 亲读Cargo.toml/Cargo.lock完整diff并用前后locked metadata核对：恰好33个本地workspace继承包2.6.16→2.6.17，外部和独立版本包、checksum及dependencies均未变化。1.93.1 metadata --locked --no-deps退出0。
- 全仓fmt、git diff --check通过；26个变更/新增Markdown的433处相对链接与requirement锚点通过。
- OpenSpec all strict为19 passed/0 failed，archive校验退出0。功能change已归档，本轮只剩本发布准备；另有三个此前已有的change不属本轮范围，保留其原状态与停止点。
- scripts/tests完整35项：34通过、1平台既有skip；含发布构建身份/门禁与exact runner的实执行、零项拒绝和超时回收对照。
- 发行说明覆盖本轮全部行为及ACP上下文字段改名，明确v2.6.15/v2.6.16未公开；没有为旧失败tag重写历史。

## 正式候选与运行

- 候选commit `0286920e96f9d4c3e09d2aaad594aacceb7786a3`，提交标题Release Grow v2.6.17 with complete paged task restoration，51个文件；干净树建立annotated v2.6.17，tag object `8b58318d88846bc7a56b02d91ef124931fc3eda5`。
- 固定Rust1.93.1的`bash scripts/validate-release.sh v2.6.17`退出0，确认版本、干净HEAD、annotated tag、notes绝对链接与发布身份/资产/门禁契约。
- 推送前复查remote main仍为bb9b4c995794db318288beddb00a2cad27e16e70，随后HTTP/1.1原子推送main与新tag；推送后remote main和tag解引用都为0286920e96f9d4c3e09d2aaad594aacceb7786a3，工作树干净。
- 从main以所有平台组及publish=true启动[正式run43943122333](https://github.com/LordCasser/grow/actions/runs/43943122333)，headSha已确认与候选完全一致。未改写旧tag。
- 本次main推送自动产生的重复Core43943122094、Windows storage43943121939、Local coordination43943121858运行已请求取消以释放runner；正式release内部同SHA的全部对应门禁仍须通过，取消项不计验收证据。OpenSpec自动检查单独保留。
- 正式run中Windows storage job133565868518已成功，Ubuntu native job133565868156已成功；OpenSpec成功，多个分发包成功。
- Windows native job133565868315在真实协调脚本473行resident load错误：Session is unavailable for a confirmed Behavior snapshot；Linux aarch64分发job133565867716的编译已成功，但原生binary同脚本同473行同错误失败。父在macOS原脚本也复现，根因与独立修复证据见[Behavior快照修复](../2026-11-17-fix-resident-behavior-snapshot/verification.md)。没有修改原断言或把重跑当作修复。
- 最终run43943122333为completed/failure。九个分发构建成功（除linux-aarch64）；Core job133565868511、Ubuntu native、Windows storage及OpenSpec成功；Windows native、macOS native job133565868500与linux-aarch64实际binary同脚本473行同错误失败。发布步骤skipped，release接口复核HTTP404，无公开资产可验证。
- 本候选保留已推送tag，不重跑旧源码公开。修复已独立验收归档，固定1.93.1新CLI原完整协调脚本10类全部PASS、确定性旧红与13项Control/caller反例全部通过、release-dist -Dwarnings及workspace/all-targets通过。具体证据见上链，不以新修复通过覆盖旧候选失败。
- 上述“保留tag、递进v2.6.18”为当时的候选策略，已被用户最新指令“确保tag发布递增，中间出的问题就撤销tag”取代。复核v2.6.15/16/17全部release接口404、本地远端对象一致后，按精确对象lease原子删除三个失败远端tag，再删除本地tag；无提交历史或正式v2.6.14变更。
- 被撤销对象：v2.6.15=45ee377a0097a58be1b739c4130e4781318b9313→5f0a8bf92c5a1c13136f06be6902969fdedf8a5d；v2.6.16=ff172088d81302ff3132be2fda5662b3d2aca641→bb9b4c995794db318288beddb00a2cad27e16e70；v2.6.17=8b58318d88846bc7a56b02d91ef124931fc3eda5→0286920e96f9d4c3e09d2aaad594aacceb7786a3。远端删除后查询为空，正式latest仍v2.6.14。失败run和原提交继续保留作为审计证据。
- 最终发行转交publish-repaired-v2-6-15：版本只按已公开v2.6.14递进一次，修复后重新创建v2.6.15 annotated tag；若此后候选仍失败，先撤销失败tag，再修复重试同patch，不再消耗版本号。

