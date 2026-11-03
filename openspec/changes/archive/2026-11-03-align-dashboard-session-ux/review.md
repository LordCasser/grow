# 独立审查

审查者未参与本轮实现。以当前未提交工作区为对象，对照本轮前 Pager 快照 `/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-ux-alignment-baseline-m4yu7kra`，不把 `git diff HEAD` 中的前序 Shell/runtime 修复计入本轮实现。依据主规范 `client-surfaces`、本轮 delta、已批准的 `analyze-dashboard-ux-alignment/design.md` 和当前调用方进行对抗性审查。业务代码只读，未操作用户真实 session、未发送外部消息、未安装或提交。

## 审查结论

发现两项 P2，均已在本轮修复并由独立审查者复核。当前未发现阻塞本轮验收的未解决问题。

## 已闭环发现

### R1 / P2：删除确认吞掉全局 Resize 更新

触发：从普通或 attached 详情打开 `/delete` / prefix D 确认，改变终端高度，再取消确认；也包括确认尚在时发起全局退出。

位置：`app/root/mod.rs::AppView::handle_input_at_with_paste_provenance` 的 `session_deletion` early return 原先位于全局 `Event::Resize` 更新之前。

后果：事件被 `SessionDeletion::handle_input` 消费，`last_known_terminal_rows`、`apply_effective_compact` 和所有 root/child 的 `note_terminal_resize` 均不执行。虽然下一帧的 modal 矩形能读取新 frame 尺寸，取消后仍会保留旧 auto-compact 与 resize 状态；这不满足已有全局退出保留 Resize、以及本轮大小变化后保持正常编辑布局的要求。

最小修正：把既有全局 Resize 状态更新放到删除 modal 输入路由之前；modal 仍独占交互按键和鼠标，不新增另一套尺寸状态。

验证建议：真实 AppView 输入顺序打开删除确认→Resize→取消，断言 app 高度/compact、root 和 child 的 resize 标记已更新；同时覆盖 exit pending 的 Resize。

状态：已修复并独立复核。既有 Resize 块现位于删除 modal 路由之前；新增 `session_deletion_and_exit_preserve_global_resize_state` 经独立重跑通过，覆盖 app 高度、auto-compact、root/child stale 标记及 exit pending 后再次 resize。

### R2 / P2：Minimal 删除确认遮住全局退出失败及恢复操作

触发：Minimal 中打开删除确认，按全局 Quit 两次，退出 suspension 返回失败或超时。

位置：`pager-minimal/src/live.rs::draw_live` 在常规 `exit_suspension` 绘制路径之前，因 `session_deletion_open` 提前调用 `minimal_api::draw_session_deletion` 并返回；原绘制入口只画 deletion。

后果：Quit 的输入路径仍全局生效，但 Minimal 始终显示旧 Delete 确认，隐藏真实 Pending/Unconfirmed 以及 Retry/Force 提示。普通 TUI 在删除确认上层画退出状态，因此两个 renderer 的全局生命周期显示不一致；失败后用户不能从画面发现恢复操作。

最小修正：删除绘制入口沿既有全局退出优先级，exit 存在时画同一退出 notice，否则画删除确认；复用原 suspension owner 与输入路由。

验证建议：通过 `/delete`→全局 Quit×2→`ExitSuspensionResolved` 失败建立真实状态，调用实际生产绘制 seam，检查 Retry/Force 可见且 Delete 确认不可见。

状态：已修复并独立复核。实际 `draw_session_deletion` 调用私有 `paint_session_deletion_or_exit`，先清理 live 区域并优先画退出 notice；`minimal_session_deletion_yields_to_global_exit_pending_and_failure` 通过真实全局 Quit 输入、dispatch 和失败 TaskResult 建立状态，在同一生产绘制 seam 验证 Pending/Retry/Force 可见、旧 Delete 不可见，并核对 Retry/Force 输入可达。

## 其他审查范围与证据

已检查：单一 SessionCommands 注册及自定义绑定、真实事件循环的 attach 后同键重投；普通/attached/child 的输入 owner 与前缀取消；Stop 的 root/background/Goal/Workflow 与 child scope；Close ACK、失败重试与迟到 binding；删除确认原 SID/cwd/epoch、新本地 writer、转忙及结果 retirement；overview read-only preview、新建 composer、搜索 query、异步 paste 回写 owner；IndexMap cycle 与过滤/分组的分离；默认身份顺序、来源/活动/current 与窄屏/CJK caret 的渲染和相应测试。对这些路径未发现额外可行动生产缺陷；未把陈旧注释或样式偏好计为缺陷。

独立运行现有 `target/debug/deps/pager-19093a5d02f274f1`，每组使用临时独立 `GROW_HOME` 和 `RUST_MIN_STACK=16777216`、`--test-threads=1`：

- `session_deletion`：3 passed。
- `ctrl_x`：19 passed。
- `search_preserves_new_session`：1 passed。
- `dashboard_child`：2 passed。
- `search_mouse_places_cjk`：1 passed。

首轮共 26 passed。R1/R2 修复后，独立检查当前生产路径和新增测试，再以相同隔离环境直接重跑最新二进制的 `session_deletion`：5 passed（旧 3 项及两项新回归），确认两项修复闭环。实现者提供的全量 Pager、minimal、构建与真实 PTY 记录仅作交叉证据，本审查者没有重新运行真实 PTY、真实 provider 或外部终端组合。静态审查不能证明所有终端的可用性。修复后只复核相关变化及回归，未扩大到无关后端架构债务。

## 归档前契约补齐复核

已复核 `Dashboard preserves mixed drop paths` 的 MODIFIED delta：四项原 Scenario 保留图片/普通路径源顺序、image cap 后仍保留文件文本、显式 file URL 降级及 deferred discard；editable peek 的目标描述改为已批准且已实现的显式新建 composer/read-only preview/独立 text-only search。与 `insert_dropped_paths`、clipboard completion target 路由及现有 mixed-drop、cap、file-URL、readonly-preview、search draft 测试一致，不引入额外行为。

删除拒绝文案增加“use Close session for drafts”，只解释现有无 session ID 草稿通过 Close 的本地 retirement 路径，不改变删除资格、ACK 或状态。上述两处均属本轮既有行为的契约/提示补齐，未新增未审实现；按要求未重复运行无关测试。
