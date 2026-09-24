## ADDED Requirements

### Requirement: Minimal frames render the selected Agent view

When a root Agent selects a child view and has no root permission pending, Minimal SHALL use that child's prompt, controls, live tail, viewport measurements and native scrollback commit frontier for the whole frame. Root permissions SHALL temporarily retain root ownership. The selected view SHALL remain the input owner and the rendering owner together; a missing or rebound child SHALL NOT cause a child input with root rendering.

#### Scenario: Selected child has different content

- **WHEN** a child with a distinct prompt, running tail and finalized history is selected from a root Agent
- **THEN** Minimal measures, commits and draws the child's content and controls without committing new root entries as the child's history.

#### Scenario: Switch away and return

- **WHEN** Minimal switches between a root and a child whose histories were each already printed in an earlier visible epoch
- **THEN** it clears the old visible screen, reprints the selected view's retained history through that view's commit frontier, and draws its own live prompt; older native terminal scrollback remains historical.

#### Scenario: Root permission interrupts child selection

- **WHEN** a root permission arrives while a child remains selected
- **THEN** input and the entire Minimal frame use the root permission owner until it resolves, then return to the selected child.

#### Scenario: Child disappears or is rebound

- **WHEN** the selected child key no longer resolves to that child view or its session binding changes
- **THEN** Minimal does not reuse the former child's committed frontier or render its content for another owner; a missing child key falls back to root input and rendering.

#### Scenario: Native clear or commit fails during a switch

- **WHEN** the terminal cannot clear for a new owner or a new owner's native block write fails
- **THEN** Minimal does not mark missing content as committed; it retries the failed boundary on a later frame and keeps the failed block in the live tail.

## MODIFIED Requirements

### Requirement: Minimal native scrollback preserves semantic lines and links

Grow minimal 模式将稳定条目提交到原生终端 scrollback 时 SHALL 以本次渲染的源行 provenance 区分软接续和硬换行，去除无语义的布局尾部填充，并保留已在 `BlockLine.content` 中的源末尾空格、可见文本、必要背景、宽字和 OSC 8 链接目标。生产者在生成 `BlockLine` 前已丢弃的源空白不属于可恢复范围。每个可见视图 epoch 的提交 SHALL 遵守 print-once frontier：终端写入报告成功后才标记条目已提交；失败时条目保持 live、布局重新测量。切换 root/child 视图后，当前 epoch 可重新打印该视图的已保留历史，旧 epoch 的打印仍保留在终端原生历史。不承诺部分终端写入后的重试恰好一次。

#### Scenario: Short structured text has no copied layout padding
- **WHEN** 短 YAML/Markdown 条目比终端宽度短并被提交
- **THEN** 原生复制内容不包含为布局填充的右侧空格或额外空行，必要的源空白仍保留。

#### Scenario: Long logical token wraps naturally
- **WHEN** 长路径或链接所在的下一渲染行由空 joiner 软接续、无重复可见装饰前缀，且前一行确实满宽
- **THEN** 终端在原生 scrollback 产生软折行，复制可得到未插入换行或补空格的原 token，即使折链跨过内部屏高分块。

#### Scenario: Decorated continuation cannot be losslessly native-wrapped
- **WHEN** 空 joiner 的续行重复绘制引用竖线、编辑路径缩进或其他不可选择的可见前缀
- **THEN** 提交保留该视觉行但保守使用硬换行，不把装饰前缀伪装成原 token 的无缝续接；该类复制不会承诺还原完整逻辑 token。

#### Scenario: Full-width source hard break is not joined
- **WHEN** 代码行刚好满宽但下一行来自硬换行，或 joiner 为一个空格/换行
- **THEN** 后一行不与前一行作为无分隔软接续，硬换行及源分隔语义保持。

#### Scenario: Wide glyph and truncation footer
- **WHEN** 行含 CJK/emoji 宽字或提交高度上限产生 footer
- **THEN** 宽字仅输出一次、列位置准确；footer 与前一行硬隔离，不继承被覆盖行的软折标记或链接。

#### Scenario: Linked text crosses a row boundary
- **WHEN** OSC 8 链接跨软折行、在边界结束或后面紧跟无链接文本
- **THEN** 终端可见字形与完整 URL/id 对齐，链接及时关闭且不泄漏到后续文字；pending wrap 不因中间控制序列丢失。

#### Scenario: Commit meets resize or writer failure
- **WHEN** 本次提交过程中终端尺寸改变，或 native writer 报错
- **THEN** 已开始的条目以一致的起始宽度计算行语义；失败条目不越过 frontier 且下一帧 live tail 重测，viewport/prompt 不多滚或跳行。
