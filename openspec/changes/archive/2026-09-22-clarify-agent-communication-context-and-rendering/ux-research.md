# UI UX Pro Max 设计取舍

本轮使用项目内 `.codex/skills/ui-ux-pro-max/SKILL.md` 及其检索脚本，目标是 Rust / Ratatui 的 agent 通信 transcript。技能库主要覆盖 Web / mobile；本项目已有终端主题、字体与键盘体系，不能把网页推荐直接当成 TUI 规范。

## 实际检索

执行 `python3 .codex/skills/ui-ux-pro-max/scripts/search.py`：

| 查询 | 参数 | 结果及使用方式 |
| --- | --- | --- |
| `developer terminal agent coordination dense keyboard first status progressive disclosure` | `--design-system -p 'Grow Agent Communication TUI' -f markdown` | 首次匹配到 Funnel / Vibrant block。保留渐进披露这一原则，拒绝转化漏斗、hero、CTA、鲜艳大块和动画建议，与终端 transcript 不匹配 |
| `developer tools dashboard minimal dense monochrome` | `--design-system -p 'Grow TUI' -f markdown` | 获得 Data-Dense / monochrome 方向；采用紧凑单列、稳定对齐和克制强调，不引入 dashboard、图表或新的全局调色板 |
| `progressive disclosure information hierarchy density` | `--domain ux -n 6` | Color Only、Heading Hierarchy、consistent type hierarchy；在终端转化为文字状态、缩进、留白和统一文本层级 |
| `keyboard focus modal escape navigation` | `--domain ux -n 6` | Focus States、Keyboard Navigation；采用明确选中态、现有键盘路由、退出后恢复 transcript 位置 |
| `loading status feedback errors recovery` | `--domain ux -n 6` | Loading Indicators、Error Recovery、Error Placement；等待有文字状态，错误放在对应交互下，恢复建议服从真实协议，未知投递不附带自动重发 |
| `table horizontal scroll text readability truncation` | `--domain ux -n 5` | Contrast Readability、Truncation、Table Handling；采用正文正常对比度、明确省略标记、完整查看/复制路径和有界表格排版 |

技术栈已明确为 Rust / Ratatui，技能没有对应 stack，未套用默认 html-tailwind。终端正文继续使用用户的等宽字体；不引入网页字体、SVG 图标、阴影卡片、px 断点或跳动的 loading skeleton。

## 第二轮原则（第三轮按下文收敛）

| 原则 | 本次具体决定 | 验收方式 |
| --- | --- | --- |
| 先让用户看见结果 | 已回答行直接显示 Q1 + A2，不要求用户展开才看结论 | 等待→已回答 fixture，结果在默认列表可见 |
| 渐进披露 | 默认行负责扫描；行内展开负责完整语义正文；详情负责长文、原文、数据与复制 | 三层拥有明确内容预算，JSON 不在默认正文重复 |
| 状态可解释 | `已回答`、`已接收`、`投递未知`、`查询完成/询问失败` 分开 | 不能把回执、主 turn idle 或重连推断为已消费 |
| 键盘优先 | 沿用 transcript / viewer 的键位和焦点恢复 | 从真实按键入口验证，不能只构造 viewer 测试 |
| 状态不靠颜色 | 工具身份、来源/去向和状态均有文字；第三轮移除常驻路由解释 | 无色终端仍能识别状态；窄宽度不丢失关键字 |
| 源文本可恢复 | 显示解析与原文复制分离，Markdown 源字段保留 | tab、CRLF、围栏、嵌套列表、URL 原样复制断言 |
| 等待不伪装成进度 | 未收到 phase 时只显示等待，Sideband 没有 token streaming 时不打字机播放答案 | parent-child / peer 证据缺失 fixture |

交互原型用于比较信息层级和阅读路径，不是 Ratatui 的像素截图，也不能代替后续 Rust renderer、PTY 和真实 clipboard 验证。


## 第三轮：以 Grow 现有 UI 为准

追加执行：

- `python3 .codex/skills/ui-ux-pro-max/scripts/search.py 'developer command line minimal monochrome existing design consistency' --design-system -p 'Grow Communication' -f markdown`
- `python3 .codex/skills/ui-ux-pro-max/scripts/search.py 'minimal interface progressive disclosure consistency status feedback' --domain ux -n 5`

design-system 返回 Minimal Single Column / Exaggerated Minimalism，但 oversized typography、hero/CTA、网页字体不适用。只采用单列和明确反馈；用户要求与现有产品代码优先。没有新建全局 design-system 实体。

核对 Grow 实际默认 bullet `◆`、选中 `›`、两格 preview 缩进、标题后两空格状态、Notice 的 COORDINATION 前缀、BlockViewer 现有圆角框/[x]、单个 ShortcutsBar。来源与行号见 tui-design.md。固定 UI 使用英文，消息原文不翻译；删去上一版常驻路由说明、每行 Enter 按钮、tabs 和专用 Y 动作。

回执确认接收已经由用户明确。意见交换和 Sideband 的选择属于运行时设计，详见 exchange-design.md；不把这些机制解释再放回常规 UI。图示中的反向意见回复是设计示意，不是 Rust TUI 运行截图。
