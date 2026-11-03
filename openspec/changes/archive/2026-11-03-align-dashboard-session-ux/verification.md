# 验证记录

## 增量 1

初始隔离基线 Dashboard 679（2 ignored）、prompt_paging 3、Ctrl+X 19 通过。移除 Dashboard Esc/Left/q 特判与 popup input 路由，普通/attached 分页统一；保留实际会话 Esc/cancel、Plan/Question 等既有 owner。旧退出测试按有意修改的契约更新；真实 DashboardAttach 构造 fixture，不再制造无生产入口的 popup 状态。新测试逐项比较普通/attached 的 vim、pane、空/非空稿、Esc/Left/Right/Tab/PageUp/PageDown/文字，检查 outcome、文本、光标、pane 和绑定页面。

编译中移除旧 guard 时发现正常 Esc hint 也复用 modal_owns_input，已保留该共同规则；更改测试诊断字串/导入时出现的编译错误已纠正，没有放宽生产分支。最终 overlay_ 139 通过、1 ignored。定向 Dashboard/分页/entry parity/显式返回/提示归属结果见 /tmp/grow-ux-detail-regressions.txt。

## 增量 2a：搜索与新建输入分离

独立 search_query 编辑 owner 保留 composer 实体，不 stash 文本替换；进入/退出搜索不会改新建文本、光标、附件或 undo。Search 的文字/粘贴/剪贴板只更新 query，Enter 应用过滤、Tab 返回新建输入、上下导航过滤结果；不再进入 prompt/attachment admission。搜索期间不显示新建输入的完成下拉。search_ 119 通过，Dashboard 680（2 ignored）通过；日志 /tmp/grow-ux-search-{second,dashboard}.txt。测试包括实际文本/图片草稿+undo 保留。

## 增量 2 完成
- 移除 overview 的第二编辑器、回答权限/问题入口和无生产入口 popup；保留只读 pending 投影，始终显示 New session composer。
- 搜索 query 独立，新增草稿/光标/附件/undo 保留回归；选择切换不改变 composer。
- 中途旧预览 API 清理误删共享投影，编译发现后恢复；清理后重跑验证。
- `RUST_MIN_STACK=16777216 GROW_HOME=<temporary> cargo test --locked -p pager --lib dashboard -- --test-threads=1`：576 passed，1 ignored（保留的手工视觉预览）。日志 `/tmp/grow-ux-overview-final.txt`。

## 增量 3 完成
- 单一 SessionCommands 注册；列表 attach/forward 到真实 AgentView，支持重绑；重复前缀仅 Stop，C Close，D 独立永久删除弹窗。
- Stop 保留真实 Goal/Workflow/children/background 控制与 Compaction cancelling 状态；Close 保留 ACK。删除绑定原 ID/cwd/epoch，重查转忙、换绑与新增本地 writer；回执不抢走后续焦点。
- Dashboard 569 passed / 1 ignored；Ctrl+X 19 passed；session_commands 4 passed；session_deletion 3 passed（焦点变化、同 ID 换绑、新本地 writer、持久确认、80×24 鼠标确认）；pager-minimal 102 passed。
- 日志：`/tmp/grow-ux-commands-dashboard4.txt`、`/tmp/grow-ux-commands-prefix-final.txt`、`/tmp/grow-ux-commands-global.txt`、`/tmp/grow-ux-commands-binding3.txt`、`/tmp/grow-ux-minimal.txt`。

## 增量 4 完成
- 详情 prev/next 使用打开的 IndexMap 顺序，不再复用总览 filter/grouping/pin；包含未发言/正在初始化的本地页。
- 总览新增默认 Open 稳定身份序，State/Directory 为显式选项；来源依据本地 row identity 与 roster resident 区分，运行状态独立显示；current 与键盘焦点、弱 hover 分离。
- New session 始终命名，输入聚焦时空 Enter/箭头不操作列表；单击选择只读预览，Enter/双击打开。新增 80×24 来源/焦点、CJK query 鼠标光标、宽字符重命名、稳定状态更新/child cluster 回归。
- 导航清理当前 root/child 前缀，包含鼠标、详情 cycle、picker 切换；80 列 guide 分成配置与管理两行。
- 清除已无生产调用的旧 reply allocator、peek bool/零高度兼容字段；保留正常 composer 几何测试。
- Dashboard 560 passed / 1 ignored；guide 1 passed；Ctrl+X 19 passed。日志 `/tmp/grow-ux-stable-final.txt`、`/tmp/grow-ux-guide-final.txt`、`/tmp/grow-ux-prefix-nav-final.txt`。

## 构建与真实入口（review 前）
- `cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce` 通过；使用 system allocator（本地默认 jemalloc 环境问题沿前序记录），sandbox 保持启用。未安装全局命令。
- `PAGER_BINARY=$PWD/target/debug/grow RUST_MIN_STACK=16777216 GROW_HOME=<temporary> cargo test --locked -p pager --test pty_e2e_config_ui dashboard_overlay_tab_esc_backout_and_ctrl_backslash -- --ignored --test-threads=1`：2 passed，`/tmp/grow-ux-pty-second.txt`。
- 实际 Mock ACP/PTY 覆盖：普通与 attached 编辑/返回；M/P/B 前缀；fork/稳定 cycle；无匹配总览过滤下继续 cycle；新建草稿经搜索/切换保留；idle 双前缀不关闭；C Close ACK；D 独立确认取消/确认；普通详情删除后的合法来源落点。
- PTY 文件断言：Close 保留 fork history；取消 Delete 保留目标；确认 Delete 移除该独立 fork；source 和前一份 closed fork history 保留。全程临时 HOME/GROW_HOME/project。
- 首轮全量 Pager：7546 passed、4 failed、15 ignored；4 失败分别为 3 条被明确替换的单击即打开旧断言、SessionCommands long_help 多行格式遗漏。已更新真实选择/双击/超时契约及多行帮助，正在重跑全量。日志 `/tmp/grow-ux-pager-full.txt`。
- 删除唯一服务旧 overview image feedback 的无调用 `text_without_image_chips` 与仅测试该死 API 的测试，消除新增 dead_code warning；正常 image/paste 管道保留。

## 独立审查与补充回归
- 独立 reviewer（非实现 agent）审查当前 diff/本轮前快照、规范、真实调用方，独立重跑 26 项已有定向回归。发现两项 P2：R1 删除 modal 先于全局 Resize，R2 minimal 删除 modal 遮挡退出 Pending/Unconfirmed。详见 `review.md`。
- 修复 R1：将既有全局 resize 更新移到删除输入 owner 之前，保留 app 高度/自动 compact 与所有 root/child resize 标记，也覆盖 exit pending。
- 修复 R2：minimal 删除绘制入口使用生产私有 paint seam，优先绘制既有 exit owner 的状态/Retry/Force；清理该 live region 的旧删除画面。
- 新增真实 Action/Input/TaskResult 到实际生产绘制 seam 的 `/delete → Quit×2 → suspension failed` 回归，及 `/delete → Resize → Quit → Resize` 全局尺寸状态回归。修复后结果待下项记录。
- 第二轮全量 Pager：7549 passed、0 failed、15 ignored，`/tmp/grow-ux-pager-full-final.txt`；随后追加 child prefix Stop/idle scope、scrollback Esc 先取消前缀 2 项定向通过，`/tmp/grow-ux-child-final.txt`。
- phase4 后 pager-minimal：102 passed，`/tmp/grow-ux-minimal-final.txt`。


## 最终核对
- 独立 reviewer 已复核 R1/R2，新增与既有删除回归 5/5 passed，当前无未解决阻塞；见 `review.md`。
- 最终源码构建通过，`/tmp/grow-ux-cli-final-build.txt`；最终构建实际 PTY 2/2 passed，`/tmp/grow-ux-pty-reviewed.txt`。帮助文案与无调用旧 API 清理已包含在最终构建。
- 归档前规范核对发现原 mixed-drop requirement 仍表述可编辑 peek；在同一 delta 明确替换为独立 new-session composer/search/readonly ownership，保留并核对原 4 个场景，图片/普通文件顺序与 cap/path fallback 断言均保留。无新的运行行为。
- 未保存草稿没有可删除历史，删除拒绝说明明确引导 Close session；未新增控制路径或存储行为。

- 最后仅拒绝说明文案补齐后的交付构建通过：`/tmp/grow-ux-cli-delivery-build.txt`。未更改后端或安装全局二进制。
- 2026-11-03 `openspec archive align-dashboard-session-ux --yes` 合入 4 个新增、3 个修改 requirement，保留所有原场景；归档后 `openspec validate --all --strict --no-interactive`：16 passed、0 failed。
- `openspec validate --archived --strict --no-interactive`：780 passed、0 failed；最终 `cargo fmt --all -- --check` 和 `git diff --check` 通过。
- 使用 Cargo 包级清理移除 33.2 GiB Pager 构建产物/缓存（含已验证的中间 test runner），保留可运行 `target/debug/grow`；清理空的隔离测试 home，日志及非空临时复核数据保留。未操作真实会话、未安装全局命令、未提交 Git。
