# U6 · 显式打开的答复编辑器拥有键盘 — 验证记录 (2026-11-07)

任务：用户从 scrollback 卡片以 `r`/鼠标显式打开本地 advisory 答复编辑器后，键盘仍被
Scrollback 消费（PTY 首轮 Down/Enter 打开了历史 viewer）。仅修此行为；不改 generic
输入路由、blocking 问答或 inbound 展示。环境：`cargo +1.93.1 --locked --offline`、
`RUST_MIN_STACK=16777216`、`CARGO_BUILD_JOBS=2`、`PATH~/.cargo/bin`。

## 生产改动（单一最小落点）

`pager/src/app/agent_view/interactions.rs::open_advisory_reply` — 在**实际成功创建**
AdvisoryReply 编辑器后（所有拒绝路径——已有 blocking view、无卡、非 Available、
Failed——此前均已早退、无焦点变化），若本 view 的 `active_pane == Scrollback` 则
`force_active_pane(Prompt)`。理由：`input.rs` 的 question-view 输入门只排除
Scrollback，编辑器虽可见但键盘死在历史区。`force_active_pane` 的契约即"协议驱动
转换、调用方已拥有生命周期决策"——此处 composer 已由本函数整体 stash（与 blocking
问一致的归属）。只写本 view 的 pane，不切 active tab，不触其他 pane/无关 view。

## 真实 old-red（先测后修）

测试 `u6_explicitly_opened_reply_editor_owns_keyboard_from_scrollback`
（`app/root/dispatch/tests/advisory_reply_admission.rs`，同 owner 既有模块新增）：
两题卡 → 卡行选中 + pane=Scrollback → **真实 `r` 键**走 `AgentView::handle_input`
→ 断言产生 `QueryAdvisoryReplyAdmission{source,call}` → dispatch → `QueryInputAdmissionStatus`
effect → 合格 durable `UnknownUnconfirmed`（无冻结正文）经现 observer 打开编辑器 →
**真实 Down/Enter 键**必须由编辑器消费（光标 0→1、Enter 在题 1 末选项推进
active_tab 0→1、`block_viewer`/`line_viewer` 保持 None——对应 PTY 打开历史 viewer
的反例）→ Esc 仅弃本地编辑：composer 草稿恢复、无 question_pending、turn-pause 不变。

- 修前：`advisory` filter **exit 101**，仅 U6 失败于 "Down must be consumed by the
  editor"（编辑器已开、键盘未归），其余 36 过。日志
  `/tmp/grow-ask-editor-focus-oldred.log`。
  （old-red 期间两次编译错误均为测试自身语法——无效 let-else、可反驳模式——已修，
  与生产行为无关。）

## 修后（green）

| 命令 | 结果 |
| --- | --- |
| `test -p pager --lib advisory --locked --offline` | **exit 0 — 37 passed / 0 failed**（U6 含内，u6 测试名单核对 `ok`）。`/tmp/grow-ask-editor-focus-green.log` |
| `… --lib question_view` | exit 0 — 57/0 |
| `… --lib turn_status` | exit 0 — 47/0 |

回归日志 `/tmp/grow-ask-editor-focus-regress.log`（逐 filter marker + 真实 exit）。
未跑全 workspace（按指示基于实际 impact）；未 fmt；未 commit。

## 边界（未 claim）

- 未宣称整体 feature 通过：fresh CLI/PTY 产品复验（b186 移除临时 Tab 后同 visible
  Reply 流程直选直提）仍 pending；本 lib 测试不替代。
- 鼠标 body-probe 后"无提交 artifact"为独立观测，不属 U6，未扩。
- 关闭编辑器后pane保留Prompt并恢复普通composer草稿。父已核这符合本次编辑完成后的用途，不增加返回Scrollback的状态或机制。
