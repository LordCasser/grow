# 通信 TUI 设计

默认记录只回答：**who, what, result**。控件、标题、状态和提示统一英文；消息原文保持原语言。上下文/Sideband/投递时机属于协议和工具说明，不在常规 UI 逐条讲解。

本轮替代上一版的常驻路由说明、每行详情按钮和正文/原文/数据页签。语义设计见 [exchange-design.md](exchange-design.md)，接收回执见 [receipt-design.md](receipt-design.md)。

## 直接沿用 Grow 的样式

以本仓库实际 Grow Pager 为依据，不另造一套“通信产品”。UI UX Pro Max 只用于检查信息密度、反馈和键盘可达性；不套用网页配色、巨型字体、卡片或导航栏。

- 工具行使用当前配置的 bullet，默认 `◆`；选中可折叠行使用既有 `›`。不额外添加彩色状态圆点或折叠三角。
- 一行工具/动作 + 参与方 + 简短状态；正文缩进两个 cell，记录之间沿用现有间距。状态紧随标题，不为此新增右对齐状态列。
- `text_primary/text_secondary/gray` 和现有 success/warning/error token；不为 peer 分配独立颜色，不把整段正文染成状态色。
- 详情复用当前 BlockViewer：同一边框、内容首行标题、右上 `[x]`、单个 ShortcutsBar。没有第二个窗口 chrome，也没有常驻 tabs。
- 保留 Notice/Tool 原有身份与手势，接收消息仍是有来源的接收记录，不伪装成接收方自己执行 send。

核对入口：`pager-render/src/appearance/config.rs:587`、`pager/src/scrollback/blocks/tool/other.rs:253`、`pager/src/scrollback/blocks/notice.rs:197`、`pager/src/app/agent_view/render.rs:3700`、`pager/src/views/block_viewer.rs:1001`，均相对 `crates/codegen/`。GrowNight / GrowDay 跟随现有主题。没有单独核验远端 Grok Build 的当前发行 UI，不以印象冒充依据。

## 默认内容

```text
◆ ask_subagent → Parser review  Answered
  Q: Should width calculation stay in the renderer?
  A: Yes. Keep parsing local and share width calculation.

◆ send_subagent_message → Parser review  Received
  Keep the parser local; share width calculation.

  COORDINATION  Message from Parser review
  Agreed. I will check the narrow-table case.
```

示例工具名沿用当前实际名称；第二阶段消息 schema 演进后再由真实工具身份生成标题，不在 UI 内虚构别名。反向回复使用真实来源身份生成英文标题。

| 内容 | 默认 | 展开 / 详情 |
| --- | --- | --- |
| 工具/动作、方向、参与方、状态 | 必须；名称过长时缩短或换行 | 完整身份可检查 |
| 等待中的 question | 最多 2 display lines | 完整 Question |
| 已回答 | Q 最多 1 行 + A 最多 2 行 | 独立 Question / Answer Markdown |
| 收发 message | 最多 2 行，不再重复 Message 标签 | 完整消息 Markdown |
| 失败 / Unconfirmed | 正文 1 行 + 原因最多 2 行 | 原错误和已有结构化数据 |
| Sideband、主上下文去向、投递模式 | 不显示 | 正文也不增加机制说明；原始参数仅在按需 Data 中可查 |
| 完整 IDs、cwd、raw JSON、receipt ID | 不显示 | 按需 Data；缺失字段不猜测 |
| 快捷键 | 只在当前焦点的单个 footer 提示 | 沿用当前绑定，不在每行复制一遍 |

状态集保持可解释：ask 使用 `Waiting / Answered / Rejected / Timed out / Cancelled / Unavailable / Failed`；send 使用 `Sending / Received / Rejected / Failed / Unconfirmed`。只显示收到的事实，不伪造阶段、百分比或 token streaming。回执不显示为 `Read/Applied/Done`。

40 列时优先保留工具/动作与状态，参与方允许单独换行；依据实际内容 cell 宽度测量，不依据中文字符个数。预览截断明确加 `…`，完整原文不截断。原文不会因为英文 UI 被翻译。

## 阅读与操作

默认扫描 → `e` 行内展开 → `Enter/Ctrl-F` 进入既有详情。行内和详情采用同一正文投影，不另造通信页面。完成态默认能读到答案，不必先展开。

详情默认正文；`r` 使用现有 raw 切换，`w` 换行，`/` 搜索，`v/V` 选择，`y` 复制选择。只在通信详情增加局部 `D:data` 查看已有协议数据，再按返回正文/原文；footer 按宽度与实际 keymap 给提示。取消上一版新增 `Y` 的决定，避免覆盖既有 metadata 复制语义；原文模式全选后沿用既有复制即可，后续需要整段复制时走统一 action，不为通信独造一套快捷键。

`Tab` 保留应用焦点切换。输入/搜索/选择状态先处理本身的按键，`Esc` 先退出当前输入/选择，再关闭详情。source/receiving tool row 双击保留展开，Parent Notice 双击保留打开详情。浏览器示意的 Tab 是网页焦点，不作为实际 TUI dispatch 验证。

新结果原位更新，不抢焦点、不跳到对方 Session、不强制把阅读者滚到底部。正在选择时延后内容替换；正文/原文/数据各自保留位置，退出返回原记录。UI 操作不产生模型输入。

## Markdown

复用现有 `MarkdownContent`，只接通通信正文入口。问题、回答、消息各自解析，结构化标题/状态独立布局；正文里未闭合的 fence 不能影响其他 section。

| 元素 | 展示 |
| --- | --- |
| 标题、列表、引用、行内代码 | 沿用 Grow Markdown 字体、缩进、层级和语义色 |
| fenced code | 现有语法高亮，未知语言按普通代码，默认软换行 |
| 表格 | 按实际可用宽度分配列宽和换行，不能默认宽渲染后截掉右侧 |
| 链接 | 保留 label/destination 的检查与用户主动操作路径 |
| 图片 | 保留 alt/路径文字，不自动加载或替换正文 |
| Mermaid | 复用已有有界终端图，失败或过宽回退源码 |
| HTML / 未支持语法 | 现有纯文本降级，无 Web HTML 执行 |

`MarkdownContent::new_inner` 会展开 tab，`text()` 不是实际收到的原文。raw 和复制取 typed body/answer 字符串，保留 tab、CRLF、围栏和缩进；终端显示仍应处理控制序列，不能让正文执行终端命令。缓存沿用 width/generation/theme/mode，同长度正文更新也推进 generation。

Minimal inquiry 在 live region 等自己的终态再打印一次；message receipt 是不可变事实，显示一次。不会因为 reply 的到来修改旧消息的 Received 状态，回复本来就是一条新的内容；也不再另打 ACK 行。

## 验收

- source / receiving / reply 三个视角身份正确；当前阶段没有 reply 能力时不能伪造其记录。
- 所有固定 UI literal 英文；原文中文、emoji、tab、CRLF 保真。
- normal/minimal/replay、40/60/100 列、GrowNight/GrowDay/无色、resize 均可读，不越界。
- 默认看到结果，未知送达不伪装失败；无路由解释行、每行按钮、常驻 tabs。
- 长代码、宽表格、未闭合 fence、同长度更新、复制与终态到达时选区保持正确。
- 实际键盘 dispatch / Ratatui / PTY 验证不能用 HTML 示意代替。
