# Replay 底部与浏览交互

状态：实施完成；实际布局、buffer 与 PTY 验收见 [verification.md](verification.md)。线框表达信息层级，实际宽度以终端 cell 测量，不能照抄字符串长度。

## 1. Skill 调查与取舍

本次按用户要求使用 [ui-ux-pro-max](../../../.codex/skills/ui-ux-pro-max/SKILL.md)，2026-09-29 执行：

```sh
python3 .codex/skills/ui-ux-pro-max/scripts/search.py "developer terminal session replay control dashboard dense information minimal accessible" --design-system -p "Grow Replay" -f markdown
python3 .codex/skills/ui-ux-pro-max/scripts/search.py "keyboard focus accessibility dense dashboard status feedback" --domain ux -n 5
python3 .codex/skills/ui-ux-pro-max/scripts/search.py "progress loading animation reduced motion responsive" --domain ux -n 5
```

设计系统命中 Data-Dense Dashboard、单栏和等宽排版；UX 检索命中可见焦点、键盘可达、等待反馈、进度可解释、减少无用动画。该 skill 数据库主要面向 Web/Mobile，未提供 Ratatui stack。本项目已有明确 Rust TUI 技术栈，不套用默认 html-tailwind。

| 检索建议 | TUI 采用方式 |
| --- | --- |
| 数据密集、减少装饰、保留上下文 | 主体仍是单栏 transcript；底部按状态、时间、操作分层，最多 5 行 |
| 可见 focus / hover | 复用既有主题 selection/hover 样式，键盘选中框必须实际绘制 |
| 键盘全可达 | 每个可点击动作有快捷键；点击区域仅覆盖实际绘制控件，不让整行误触 |
| 加载反馈 | 原终端阶段提示；播放积压时明确“追赶记录”，不靠假百分比 |
| 可解释进度 | 百分比和耗时使用压缩后的全局播放轴；保留原历史时间并标估算 |
| 减少动画 | 只保留用户需要的正文模拟流式；footer 不闪烁、不跑马灯、不做 pulsing/渐变动画 |
| 响应式与对比度 | 按列/行数降级，复用 Theme；Playing/Paused/Finished 必有文字，不仅靠颜色 |

不采用检索出的网页 Hero/居中 CTA、黑金新色板、Google Fonts、SVG/emoji 图标、CSS easing。终端字体、窗口尺寸和颜色能力属于用户环境；沿用 Grow 的主题、边框和宽度处理，不为 Replay 建立另一套全局设计系统。

## 2. 布局

布局只划分 `transcript area + replay panel`，顶部不再额外占一行放重复 Replay 标题。常驻 panel 中的第一行兼作分隔，不额外预留 composer padding。详情覆盖正文区域，panel 保持可见。

标准布局：宽度至少 100 列且高度至少 24 行，底部 5 行。最多 5 行是上限，不因标题长度或历史状态数量增高。

```text
│ 正常 transcript：消息、工具组、子 agent 卡片、选择框和滚动条                         │
│ …                                                                                │
├ Replay · root › discovery-2                   只读 · 模拟流式                      │
│ [F8 暂停] [-] 16× [+]  ========------------  38%   跟随播放                         │
│ 历史 2026-09-29 11:49:55 UTC+08:00   原时长 02:12:00   回放 00:09 / 00:26           │
│ 跳过 1h 23m 10s IDLE 时间 · 按 1s 重放                                             │
│ Enter 详情  ←/→ 折叠  / 搜索  ] 下一记录  Esc 返回  ? 帮助                          │
```

常态第四行显示 `历史 Behavior: Plan · Goal: paused · 截至快照未完成: 2` 等当前节点已知状态；IDLE 发生时该行切换为跳过说明，之后保留“上次跳过 …”。被暂时让出的历史状态可在 `?` 的状态摘要查看，正文原事实照常显示，不轮播、不自动消失。警告优先于普通状态；多个告警汇总数量并提供摘要，不反复顶起 panel。

第 3 行的“回放”表示当前 P / 全局总 P，不是本次实际运行的墙钟耗时，也不是剩余时间预测；speed 变化不会修改分母。Finished 时为总时长/总时长。原时长是可靠来源首尾的差，缺失则显示未知/估算，不能用读取当前时间补足。

80×24 使用 4 行，将 breadcrumb 与播放摘要合并，完整历史时间独立一行，省略图形进度条和原时长。必要字段顺序：Replay/播放状态 → 倍速 → 进度 → 当前节点短名；长路径从中间省略，保留根和当前节点。

```text
│ Replay · Paused · 16× · 38% · root › … › discovery-2                          │
│ 历史 2026-09-29 11:49:55 UTC+08:00 · 回放 00:09/00:26                         │
│ 上次跳过 1h 23m 10s IDLE 时间 · 按 1s 重放                                   │
│ F8 继续  Enter 详情  Esc 返回  ? 帮助                                        │
```

60×15 使用 3 行；保留状态/倍速/百分比、历史时间、核心操作。发生跳过时第 2 行显示跳过提示，完整历史时间和当前路径进入 `?` 状态摘要；结束后保持上次跳过提示，不把高倍速下的信息一闪而过。

```text
│ Replay · Playing · 16× · 38%                         │
│ 跳过 1h 23m 10s IDLE 时间 · 1s 重放                  │
│ F8 暂停  Esc 返回  ? 帮助                            │
```

精确尺寸规则：先按宽度选择 5/4/3 行（>=100 / >=72 / <72），再按高度限制：>=24 可用标准高度，16–23 最多 4 行，10–15 最多 3 行，6–9 最多 2 行，<6 最多 1 行且优先退出提示。宽或高为零时不绘制和不保留热区；height=1 时唯一一行用于退出提示，正文为零行。height>=2 时 panel 不超过 `height.saturating_sub(1)`，正文至少留 1 行；标准尺寸（>=100×24）正文至少 5 行。其余尺寸严格按宽度档与高度上限取较小值，不另设有冲突的最小高度。40 列以下只保证状态短词和 `?`/退出可发现，其他信息在只读帮助内查看。

**不直接 truncate 一条长 status string。** 分字段按 unicode display width 布局，优先级低的字段整体省略，日期与时区不切成误导的半截。CJK、组合字符、emoji、长 title 都必须参与布局测试。resize 后重算正文和 panel rect，清除旧 hover/press/drag 热区。

## 3. 状态与控件

| 状态 | 主控件 | 进度与辅助说明 |
| --- | --- | --- |
| Preparing（接管终端前） | Ctrl-C 退出 | 按阶段显示“验证来源 / 准备 N 个节点”，不显示未经计算的百分比 |
| Playing | F8/Space 暂停 | 展示 speed 和全树进度；当前节点已播完但其他节点未完不能显示 Finished |
| Paused | F8/Space 继续 | 所有历史 elapsed、reveal、进度冻结；浏览仍有效 |
| Catching up | 仍可暂停 | “追赶记录”，显示已交付进度，不显示目标时钟伪进度 |
| Finished | 暂停/继续禁用 | 100%；“播放结束，可继续浏览”；未结束的历史业务行仍保留 |
| Unknown timing | 保持普通控制 | “时间估算”，可显示相对 P；不把 UTC fallback 当作时间来源可靠 |

图形进度条是指示器，没有 click-to-seek 行为或可拖动的把手。点击 pause/speed/back/follow/下一记录与相应快捷键使用同一 action。disabled 控件同时给文字/样式，点击不改变状态；不对整个 panel 捕获正文滚动。panel 的显式点击不受正文局部字符焦点影响；即使详情/搜索打开，仍可点击暂停、倍速和下一记录，操作后保留原局部焦点。

选中详情、搜索、子页切换均不隐式暂停。查看时想暂停可用 F8 或 panel 按钮；用户主动暂停后，关闭详情不自动恢复。鼠标跟随开关和 End 只改变 viewport，不移动播放时钟；播放继续但用户离底阅读时显示“正在浏览历史 / End 跟随”。

## 4. 键盘与鼠标

| 范围 | 输入 | 行为 |
| --- | --- | --- |
| 全局 | Ctrl-C | 只退出 Replay，恢复终端 |
| 全局播放中/暂停 | F8 | 暂停/继续，不受详情搜索字符输入影响 |
| 正文，无局部编辑 | Space、+/- | 暂停/继续、倍速 ×2 / ÷2；沿用有限正数边界 |
| 正文 | ↑/↓、j/k、PgUp/PgDn、Home/End | 滚动；End 回底并跟随 |
| 正文 | Tab/Shift-Tab | 按条目选择，绘制焦点框；不悄悄改变 ↑/↓ 的滚动语义 |
| 正文选中条目 | Enter/Ctrl-F | 按条目类型打开详情/子页；组头展开/收起 |
| 正文 | ←/→ | 收起/展开所选组或内容 |
| 正文 | /、n/N | 打开当前节点全文搜索，定位已投影结果；不搜索未来队列 |
| 正文 | y、拖选 | 复制选中正文/文本选区；失败明确反馈，不吞错误 |
| 正文 | ] | 下一记录；全树最近待交付事件，保留 Playing/Paused，提示手动跨过的时间；详情/帮助不把此键作为全局快捷键，搜索编辑内是普通字符 |
| 正文 | ? | 打开只读帮助与当前状态摘要，包含被窄屏省略的字段 |
| 详情 | /、v、y、w、r、data toggle | 沿用 viewer 原有搜索/视觉选择/复制/换行/raw/data 行为，按实际 viewer 能力提示 |
| 搜索编辑 | 字符、Space、+/-、q、粘贴 | 仅进入本地搜索；不会触发播放动作或 session prompt |
| 搜索编辑 | Esc | 结束查询编辑，保留结果；q 仍是查询字符 |
| 非编辑搜索结果/纯选区 | Esc/q | 先关闭结果层或清除选区，后续按键才关闭详情或返回；根页纯选区的 q 不退出播放器 |
| 详情/帮助 | Esc/q | 关闭当前层，回原节点阅读位置；正在搜索输入时 q 是字符 |
| 子页正文 | Esc/q、面包屑返回按钮 | 返回父页，保留位置；根页才退出 |
| 正文鼠标 | 滚轮、单击/双击、拖动 | 复用允许的被动内容动作；group header 与 member 命中独立，文本拖选不打开详情，外部 opener/命令不在允许动作中 |
| panel 鼠标 | 明确按钮 click | pause、speed、follow、返回、下一记录；press/release 同一有效热区才生效 |

`ShortcutsBar` 只画 hint，不自带 mouse hit-testing。Replay 为实际按钮单独保存 rect；键表与提示从同一份有限动作映射生成，避免“提示存在但不能操作”。详情态提示必须来自当前 viewer 能力，不能对所有 viewer 宣传不存在的 wrap/raw。Space 在详情内遵从 viewer 的原行为，panel 此时显示 F8 暂停而非 Space。

搜索可能在后台完成；结果需核对当前 session/node、entry generation、query generation。切换子页、关闭搜索或新查询后，旧结果不能抢焦点或跳到别的节点。内容流式追加不在每个 frame 重建全量索引；打开搜索/提交新查询时同步已显示内容，提示其为当前查询快照，重按 Enter 刷新。

## 5. 视觉与可访问性验收

复用主题样式：普通文字、次要文字、accent、selection、warning，不硬编码截图中的黑色。Playing/Paused/Finished、未知时间、只读、模拟流式都有文本；配色只作辅助。正文区的光标/选择框、详情滚动条和 footer hover 保持一致，不加新的 blinking 状态灯。

至少核对 120×40、100×24、80×24、60×15、40×8、极小尺寸；深浅主题；中文长标题；长时间跨度；详情打开时 resize；鼠标 press 后流式追加/缩放/切页；单击空白；无鼠标环境纯键盘；Playing/Paused/Finished/IDLE/估算/未知状态。

组件级方案与线框由真实 Ratatui buffer 测试和 PTY 操作核对，证据列在 [verification.md](verification.md)；网页 mockup 不构成终端行为验收。
