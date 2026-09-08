## ADDED Requirements

### Requirement: Inline terminal synchronized wrapper
with_synchronized_output SHALL 排队Begin，执行闭包，再排队End并flush，最后返回闭包结果。

#### Scenario: 实现边界
- **WHEN** 闭包返回Err或panic
- **THEN** Err仍尝试结束，结束/flush错误可覆盖原错误；panic无恢复guard，不提供事务写入。

证据：`crates/codegen/ratatui-inline/src/common.rs` — `with_synchronized_output`。

### Requirement: Inline legacy scrollback emission
emit_to_scrollback SHALL 按真实宽分段，清viewport下方并打印内容、补viewport高CRLF，flush后reset back buffer及更新y。

#### Scenario: 实现边界
- **WHEN** 空内容或无效尺寸
- **THEN** 空内容仍执行清理；尺寸假设及len转u16未全面检查，不承诺任意尺寸安全。

证据：`crates/codegen/ratatui-inline/src/scrollback.rs` — `emit_to_scrollback`。

### Requirement: Inline legacy history rebuild
resize_purge_rerender SHALL 发送2J、3J、H，重放完整history并补viewport空行，用LF快判或分段计算新y，再setarea和clear。

#### Scenario: 实现边界
- **WHEN** 实现与RIS注释不一致
- **THEN** 以实际清屏/清history序列为准，不发送RIS；失败不回滚此前输出。

证据：`crates/codegen/ratatui-inline/src/resize.rs` — `resize_purge_rerender`。

### Requirement: Inline legacy viewport height validation
独立resize_viewport_height SHALL 高度相同直接成功，否则拒绝0及不小于屏高；增长先用下方空间再向上滚动，缩小保持y。

#### Scenario: 实现边界
- **WHEN** 与Terminal方法比较
- **THEN** 该辅助函数的合法性检查不同，不可互相替代推导边界。

证据：`crates/codegen/ratatui-inline/src/resize.rs` — `resize_viewport_height`。

### Requirement: Inline ANSI borrowed segmentation
split_into_line_segments SHALL 按anstyle解析事件产生借用切片，LF以Display补CRLF，剔除紧邻CR，裸CR重置宽度。

#### Scenario: 实现边界
- **WHEN** 输入有光标移动、tab或退格
- **THEN** Other只扩字节范围，不模拟终端位置；分段不是终端模拟器。

证据：`crates/codegen/ratatui-inline/src/segment.rs` — `split_into_line_segments`。

### Requirement: Inline character width wrapping
分行 SHALL 按Unicode字符宽累加，超过term_width且已有visual时分段；ANSI本身不计宽。

#### Scenario: 实现边界
- **WHEN** 单字符比屏宽还宽或宽度零
- **THEN** 不拒绝，可能产生超宽段；非grapheme算法，不能推导字素或终端reflow完全一致。

证据：`crates/codegen/ratatui-inline/src/segment.rs` — `split_into_line_segments`。

### Requirement: Inline incomplete escape handling
分行 SHALL 只包含完成事件认领的尾字节，LF可中断待完成转义，尾非visual段在连续且无CRLF时合并。

#### Scenario: 实现边界
- **WHEN** 输入尾部有未完成转义
- **THEN** 可能省略未完成字节，不保证任意ANSI文本无损。

证据：`crates/codegen/ratatui-inline/src/segment.rs` — `split_into_line_segments`。

### Requirement: Inline viewport initialization
Terminal with_options SHALL 对Fullscreen/Inline读取backend尺寸，Fixed直接使用给定rect；分配双cell/link层，Inline根据cursor预留行。

#### Scenario: 实现边界
- **WHEN** Inline高度大于屏高
- **THEN** compute_inline_size限实际height，但append_lines基于请求高度和旧offset，不是完全无副作用构造。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `with_options`。

### Requirement: Inline frame construction boundary
get_frame SHALL 从当前buffer、viewport和frame count构造Frame；本fork通过OurFrame与Frame的unsafe转换接缝实现。

#### Scenario: 实现边界
- **WHEN** 依赖Frame内部布局变化
- **THEN** 仅检查size，不提供字段布局一致性验证；调用方不能据此宣称任意版本兼容。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `OurFrame`。

### Requirement: Inline plain frame diff
flush SHALL 使用usize索引计算diff坐标，考虑cell变化、宽字符invalidated和Skip标记，交给backend draw并返回是否有更新。

#### Scenario: 实现边界
- **WHEN** backend draw失败
- **THEN** last_known_cursor_pos可能已经更新；flush函数本身不调用Backend flush。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `diff_large`。

### Requirement: Inline frame hyperlink layer
set_frame_links SHALL 重建当前link层，裁剪绝对viewport坐标，后输入的重叠span覆盖之前span。

#### Scenario: 实现边界
- **WHEN** 同URL多个span
- **THEN** 表不去重，diff通过解析后的URL和id比较，不只比较表下标。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `set_frame_links`。

### Requirement: Inline hyperlink aware diff
flush_with_links SHALL 对glyph/style或链接变化重绘，双帧无链接时走普通flush；按相同目标分组发出OSC8。

#### Scenario: 实现边界
- **WHEN** 链接被移除但文字相同
- **THEN** 重绘无链接文字清除旧链接；普通draw不自动调用此路径。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `flush_with_links`。

### Requirement: Inline OSC8 failure handling
write_osc8_open SHALL 过滤URL控制字符，使用BEL结束并可附数字id；linked draw之后尝试close再返回draw错误。

#### Scenario: 实现边界
- **WHEN** open或close写失败
- **THEN** 仍可能留下部分序列，close失败可覆盖draw错误，不能保证绝无悬挂链接。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `emit_frame_with_links`。

### Requirement: Inline resize policy
resize SHALL 将原先y0覆盖旧屏高的Inline铺满新area，其余Inline按cursor偏移重算；autoresize仅处理Fullscreen/Inline。

#### Scenario: 实现边界
- **WHEN** 手动resize Fixed
- **THEN** 仍使用传入area；setarea先于clear，失败无状态回滚。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `resize`。

### Requirement: Inline draw lifecycle
try_draw SHALL autoresize、执行callback、普通flush、设置cursor、swap、Backend flush，成功后递增wrapping frame count并返回CompletedFrame。

#### Scenario: 实现边界
- **WHEN** callback或最终flush失败
- **THEN** callback失败可能已有resize和buffer修改，最终flush失败发生在swap之后；不是原子提交。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `try_draw`。

### Requirement: Inline cursor restoration
cursor方法 SHALL 在backend成功后更新本地状态；Drop只在hidden时尝试show_cursor并忽略错误。

#### Scenario: 实现边界
- **WHEN** 销毁Terminal
- **THEN** 不恢复raw mode、alternate screen或其它终端模式。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `hide_cursor`。

### Requirement: Inline clear and buffer reset
clear SHALL 按Fullscreen全清、Inline从viewport位置往后清、Fixed逐行从x0往后清，成功后reset back cell和link层。

#### Scenario: 实现边界
- **WHEN** Fixed rect不是整行
- **THEN** 清除范围可超出rect；reset_back_buffer不发终端命令，swap同步清旧back links。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `clear`。

### Requirement: Inline explicit area mutation
set_viewport_area SHALL 调整双buffer并清双link层，保存新area；last_known_area读取最近记录的全屏area。

#### Scenario: 实现边界
- **WHEN** 调用方越过set_viewport_height直接改area
- **THEN** 不更新保存的Inline高度或backend屏幕尺寸，需调用方保持几何一致。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `set_viewport_area`。

### Requirement: Inline viewport growth preservation
Terminal set_viewport_height SHALL 仅对Inline生效，以实际area高度判断增长，覆盖屏幕下缘时先滚动再调整y并clear。

#### Scenario: 实现边界
- **WHEN** 新高度为零或超过屏高
- **THEN** 没有独立辅助函数的拒绝校验；保存的Inline高度先更新，后续错误不回滚。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `set_viewport_height`。

### Requirement: Inline insertion callback semantics
insert_before_with_links SHALL 先分配height乘viewport宽buffer并调用closure，再按viewport和feature选输出路径，链接坐标相对插入buffer。

#### Scenario: 实现边界
- **WHEN** viewport不是Inline
- **THEN** 仍执行closure但不输出并返回成功；不等同实际插入。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `insert_before_with_links`。

### Requirement: Inline insertion without scrolling regions
默认插入 SHALL 以屏高分块，使用append_lines向上滚动后绘buffer，最后移动viewport并clear。

#### Scenario: 实现边界
- **WHEN** 终端部分写入失败
- **THEN** 已写内容不回滚；零屏高或无效超高viewport不具循环进展保证。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `insert_before_no_scrolling_regions`。

### Requirement: Inline insertion with scrolling regions
scrolling-regions 插入 SHALL 对满屏viewport逐行借顶部写入并上滚，最后恢复顶部；有下方空间先下推viewport，再在上方region分块上滚。

#### Scenario: 实现边界
- **WHEN** 恢复满屏顶部行
- **THEN** 恢复采用无链接draw helper，不能据此保证原顶行链接层完全重发。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `insert_before_scrolling_regions`。

### Requirement: Inline insertion hyperlink selection
draw_lines_with_links SHALL 按source_row映射链接，跳过宽字符续cell，每行按首个覆盖span分组draw，块尾flush。

#### Scenario: 实现边界
- **WHEN** 重叠span
- **THEN** 插入路径取首个，与frame layer后项覆盖规则不同；分块宽使用last_known_area，依赖与插入buffer宽一致。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `draw_lines_with_links`。

### Requirement: Inline cleared row optimization
scrolling-regions 无链接清空行绘制 SHALL 对空buffer做上游diff；有链接则用完整行链接输出。

#### Scenario: 实现边界
- **WHEN** 超大坐标或非零y_offset
- **THEN** 该helper仍依赖上游diff及自身area构造，不由diff_large保护所有路径。

证据：`crates/codegen/ratatui-inline/src/terminal.rs` — `draw_lines_over_cleared`。

### Requirement: Inline segmentation differential fixtures
差分测试 SHALL 以此前termwiz实现为参考，比较固定样本和2000份固定seed拼接输入的切片与CRLF标志。

#### Scenario: 实现边界
- **WHEN** 测试通过
- **THEN** 只证明所选输入/宽度一致，不证明真实terminal模拟、任意C1或不完整转义一致。

证据：`crates/codegen/ratatui-inline/tests/segment_differential.rs` — `randomized_ansi_soup_matches_reference`。

### Requirement: Inline interactive example
inline example SHALL 演示普通/高内容输出、增减viewport、resize历史重放及100ms spinner，并处理q/Esc/Ctrl-C退出。

#### Scenario: 实现边界
- **WHEN** 示例返回IO错误
- **THEN** 正常末尾恢复路径可能跳过；panic hook只尝试关闭raw mode，示例不是产品保证。

证据：`crates/codegen/ratatui-inline/examples/inline.rs` — `main`。

### Requirement: Inline rendering benchmarks
bench SHALL 包含80列文本分段以及256乘100的plain、无链接和50链接flush基准。

#### Scenario: 实现边界
- **WHEN** 基准输入标为plain或colored
- **THEN** 颜色由auto生成且plain仅有限字符串替换；未运行基准不得宣称性能结果。

证据：`crates/codegen/ratatui-inline/benches/bench.rs` — `bench_flush_with_links`。
