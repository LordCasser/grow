# v2.6.17 发布验收

## 发行身份与前置事实

- 正式最新版API返回v2.6.14，published_at=2026-11-15T17:57:30Z，非draft/非prerelease。
- remote main仍为bb9b4c995794db318288beddb00a2cad27e16e70；v2.6.17远端tag查询为空。本轮尚未创建候选tag。
- v2.6.16官方run43925904022最终failure，publish skipped，release API404。Windows两包dead-code、Core Tasks恢复和macOS runner容量原因分别留证于[失败候选](../archive/2026-11-17-prepare-release-v2-6-16/verification.md)。v2.6.15同样为未公开失败候选，旧tag均保留。

## 当前验证状态

分页Tasks恢复的索引、Shell和Pager模块均已亲读验收，并已[归档](../archive/2026-11-17-fix-paged-task-history/verification.md)。固定Rust1.93.1验证：默认CLI构建及原失败Tasks冷恢复PTY通过；Shell4877/Pager7735共12612项库回归通过，29项既有ignored；完整官方38项PTY按target/name/顺序比对一致且每项exact通过；正式release-dist -Dwarnings与workspace/all-targets通过。格式、相对链接/requirement锚点、OpenSpec严格及归档校验通过。

这些行为验证发生在最终产品源码、版本递进前（metadata=2.6.16）。后继2.6.17只同步本地版本及说明，不重复产品实现；正式同SHA全平台/公开资产仍待本记录追加，不把本地通过等同正式发行完成。

## 版本与候选静态检查

- 亲读Cargo.toml/Cargo.lock完整diff并用前后locked metadata核对：恰好33个本地workspace继承包2.6.16→2.6.17，外部和独立版本包、checksum及dependencies均未变化。1.93.1 metadata --locked --no-deps退出0。
- 全仓fmt、git diff --check通过；26个变更/新增Markdown的433处相对链接与requirement锚点通过。
- OpenSpec all strict为19 passed/0 failed，archive校验退出0。功能change已归档，本轮只剩本发布准备；另有三个此前已有的change不属本轮范围，保留其原状态与停止点。
- scripts/tests完整35项：34通过、1平台既有skip；含发布构建身份/门禁与exact runner的实执行、零项拒绝和超时回收对照。
- 发行说明覆盖本轮全部行为及ACP上下文字段改名，明确v2.6.15/v2.6.16未公开；没有为旧失败tag重写历史。

