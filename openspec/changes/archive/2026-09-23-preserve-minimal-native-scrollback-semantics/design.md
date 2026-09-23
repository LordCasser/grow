## Context and evidence

`pager-minimal/src/commit.rs::commit_leading_run` 以稳定条目和 `commit_h` 控制 print-once frontier，在 writer 成功后才 mark committed。`pager-minimal/src/lib.rs` 在失败后重新测量 live tail。当前 `ratatui-inline/src/terminal.rs::insert_before_with_links` 将每个 Buffer cell 画入原生滚动区；链接由 LinkSpan 的本地坐标另加一层。`pager/src/scrollback/wrappers/entry_renderer.rs` 可取到 `BlockOutput.lines[*].joiner`，`full_view.rs` 有可复用的字形/SGR/OSC8 序列化思路，但 transcript 输出独立。归档 `client-surfaces` 未规定原生复制语义。

## Decisions

1. **源 provenance 而非宽度猜测。** `EntryRenderer` 为本次 width、显示模式和布局导出逐物理行的软接续资格与 `BlockLine.content` 占用终点，并计入 vpad、group header、skip_rows。仅下一行 `joiner == Some("")` 且无重复可见装饰前缀时才可能无分隔接续；`None`、`Some(" ")`、`Some("\n")` 均不能当作软接续。引用竖线及编辑路径缩进虽有空 joiner，仍不能在保留原视觉时产生无污染的原生复制；首版保守用硬行，不重构这些块的呈现。刚好填满屏宽不是软折行的充分条件。若 provenance 无法映射，则保守输出硬换行。
2. **语义行只作为写入边界值。** commit 仍按 `full_h` 布局、裁到 `commit_h`，把 offscreen Buffer、源占用终点与 LinkSpan 序列化为按顺序的行记录，包含可见字形/样式/链接、是否占满宽和下一行软接续。末尾空白以 `BlockLine.content` 已保留的列为界，不从 Buffer 空格猜测它是源内容；只裁掉未被内容/样式占用的布局 pad。生产者在生成 `BlockLine` 前已经丢弃的源空白无法在 commit 恢复。宽字 continuation cell 不重复输出，代码或 diff 有意义的背景/空白保持可见。`row_count == commit_h`，cap footer 自成硬行，切断前一软接续并裁掉被覆盖行的旧链接。
3. **vendored terminal 是原生协议所有者。** 新增与现有带链接 API 平行的语义插入方法，复用 viewport 位移、滚动区与跨屏分块。此 API 必须建立并保持 DECAWM=on 的终端所有权：未知的继承状态无法查询/恢复，而原生 WRAPLINE 在 DECAWM=off 时不可能产生；入口显式启用，出口（含错误）保持启用，不能声称恢复未知先前值。只有行确实满宽且源下一行软接续时让终端 autowrap 产生 WRAPLINE；短行即使源无分隔也硬换行，不为制造软折行写补齐空格。满宽硬行通过局部 DECAWM 控制防止下一行意外接续。跨 chunk pending-wrap 必须在定位/SGR/OSC8 等控制序列前正确消耗；链接以可见字形列坐标合成，完整 URL/id 保留，离开链接及错误出口 best-effort 关闭 OSC8 并回到 DECAWM=on。`scrolling-regions` 有效和无效两种编译配置均不能静默丢行。
4. **不改变 frontier 的真相。** 写入成功才标记已提交；I/O 错误保留条目在 live tail，并按现有流程重测、重试。终端部分接收后缺少 ACK，重试可能造成重复，不能承诺端到端 exactly-once。resize 以开始 commit 时的单一 width 完成此次语义映射；下一帧按新 width 重算未提交部分。

## Risks / trade-offs

- 某些终端对 pending autowrap、OSC8 和 scrolling region 的组合有差异；使用 PTY/VTE 验证字节与真实复制结果，不能只看截图。
- 背景着色到行尾是否语义空格需以 BlockOutput 的内容/样式界限判断；不允许简单 `trim_end` 抹掉代码/差异块的可见语义。
- writer 会实际发出 `BlockLine.content` 仍持有的尾随空格，但终端原生选择对行尾空格是否裁剪由终端自身决定；不能把序列化单测等同于所有终端的复制保证。
- 超过屏高的软折链、恰满宽行和 footer 边界容易导致多滚一行；用分块/最后一行测试阻止 viewport 或 prompt 跳动。

## Verification and rollout

先对 joiner→row provenance 与序列化作纯单测，再扩展 `ratatui-inline` RecordingBackend 测试观察字节/滚动量，最后至少运行一项真实 PTY/VTE 复制与 resize/OSC8 场景。`/transcript` 仍走自己的完整正文路径，必须回归以确认没有被新 native writer 改写。记录所有实际命令与失败，完成后归档。
