# ratatui-inline 逐文件审阅记录

最终状态：10份Rust文件4078行及Cargo.toml已全文阅读，27项功能已映射inline-terminal-runtime；分段日志的待办以后文闭合记录为准。默认配置测试已通过，全特性测试进行中。

## 已确认行为与证据边界

- manifest 的 scrolling-regions 转发 ratatui feature，生产 ANSI 分行使用 anstyle-parse，termwiz 仅dev差分参考。lib导出TerminalLike、同步包装、旧resize/scrollback辅助、分行器以及Terminal/LinkSpan。
- with_synchronized_output：Begin失败不调用闭包；闭包返回Err仍尝试End和flush，但后两者失败会覆盖闭包错误。panic无恢复guard，同步控制码不是事务性IO或真正原子操作。唯一测试检查marker/text/flush数，不注入失败。
- emit_to_scrollback：取真实size，分行，清viewport往下、原样打印segment并补viewport高CRLF，再清新区域；flush成功才reset back buffer和更新viewport。空内容仍清/补行；分段len先as u16及普通加减，假定有效viewport/尺寸，仅debug断言bottom，极大history或零高不具稳健保证。测试是Mock捕获，颜色测试仅找文字，不严格验证SGR序列。
- resize_purge_rerender实际发送2J、3J、H而非文档所说RIS；完整重放history并留viewport空行，用有上限LF计数快速判断，否则分段估行。先输出flush再setarea和clear，非事务，视口高度未按新size强制缩小；测试检查普通尺寸输出和位置，不证明所有terminal reflow一致。
- 独立resize_viewport_height先取size，若高度相同直接成功（先于合法性校验），否则拒0及>=屏高。长高先用下方空余再向上推，通过底部CRLF滚动，清旧区域、设置新rect；缩小保持y。不要混同Terminal同名近似方法的边界。
- split_into_line_segments：返回借用原字符串切片，LF标准化Display为CRLF、紧邻CR剔除，裸CR重置列宽；unicode char宽累加而非grapheme。超宽单字符可以单独超过term_width；零宽不被拒绝。SGR/OSC/cursor等Other只扩字节范围，不模拟光标移动、tab和退格。未完成尾转义可丢弃，不是任意ANSI无损解析器；LF可在转义中触发并舍未完成部分。尾非visual合并需连续切片。
- differential tests：23个固定样本×9宽度，加2000个固定seed ANSI拼接；仅比较此前termwiz分段结果。完整转义/无UTF8 C1是声明边界，非终端模拟或fuzz穷举。src/segment单测覆盖普通换行、CR、SGR、CJK、宽字符、空输入。

## terminal.rs 1–1120

- OurFrame与ratatui Frame通过unsafe transmute转换，仅assert size一致，无法据此证明字段布局一致；公开结构私有字段但get_frame依赖此接缝。相关架构债务需单列，不在文档迁移修复。
- new默认Fullscreen；with_options按fixed或backend大小分配双Buffer及link层，Inline调用compute_inline_size（后文待读）。Drop仅尝试恢复hidden cursor，忽略错误，不恢复raw/alt等模式。
- flush以diff_large比较，先更新last_known_cursor_pos再backend.draw；返回有无cell改动，本身不是backend flush。set_frame_links清当前层，裁viewport边界，span逐个注册，不去重，相交后项覆盖前项；实际链接值参与diff而非只比较表内序号。
- flush_with_links两帧link table均空时走普通flush，否则链接层参与diff后emit。OSC8 URL过滤控制字符、BEL终止、可附数值id；普通draw/try_draw仍走flush，不自动走link-aware路径。
- resize对从y0覆盖旧屏高的Inline直接铺新area，其它Inline按原cursor偏移重算，Fixed手动resize也改area；autoresize只处理Fullscreen/Inline。先setarea再clear，成功后才last_known_area，失败不回滚。
- try_draw先autoresize，再callback；callback Err不flush，但可能已resize/clear且当前buffer已修改。成功路径flush、设置cursor、swap、Backend::flush，最后才计frame_count wrapping_add；末flush失败时已经swap。多处doc例子引用的是上游ratatui::Terminal，不能全部作为本fork覆盖证据。
- clear：Fullscreen全清；Inline从viewport位置AfterCursor；Fixed逐行从x0 AfterCursor，范围可能超出fixed rect；成功才reset back cells和links。reset/swap同步处理link层。cursor字段在backend成功后更新。
- insert_before_with_links在判断viewport种类前先分配height×viewport_width buffer并调用closure；非Inline仍执行closure但不输出。链接坐标相对插入buffer，随后按feature选择算法。
- Terminal::set_viewport_height只处理Inline，以实际area height判断grow/shrink，先改保存的Inline高度，clear，再按溢出滚动全屏/append，setarea后再clear；未像独立辅助函数拒0或>=屏高，失败无回滚，y+height普通u16相加。
- 无scrolling-regions插入路径按screen高分块、滚动、输出，再setviewport/clear，使用i32中间数；若零屏高或viewport大于屏高可能没有循环进展，需另行验证。不是terminal部分写失败事务；本轮读至有regions路径开头，实际分块/link发出/diff算法待后续全文。


## 全文读取闭合

terminal.rs1121–1747、src/tests.rs479行、benchmark206行、example310行均完成阅读，共10份Rust文件4078行及manifest。

regions插入满屏分支逐行借顶部、上滚并恢复旧top line，恢复走无链接helper；非满屏先向下推viewport，然后循环上滚上方region。绘插入行以last_known_area.width切片而buffer按viewport宽分配，几何不一致会失败；无链接路径wide skip跨整chunk，有链接每行重置。插入重叠链接取首个，frame层取最后一个，不可混同。linked draw失败后尝试close但close本身可失败。无链接cleared helper使用上游diff，且Rect.height构造成y_offset+lines_to_draw，不能把主diff_large保证扩展到此处。

主diff以usize坐标除法避免flat index截断，仍要求双buffer同area；跳过cell和宽字符invalidated策略不是任意外部buffer验证。compute_inline_size按请求高度append_lines，再限实际height；setarea不校验几何且不更新Inline保存高度。

测试backend多数忽略真实坐标/clear/scroll，只记录字符和累计滚动行数。链接测试覆盖增加/移除/重定向/非原点/CJK/完整目标，以及两个grow场景；未覆盖所有IO失败、超大屏、regions满屏恢复链接。resize六项测试只验证area。部分doc测试引用上游Terminal，需分开计证据。

example混合Unicode/ANSI、CRLF历史、事件递归poll和spinner；窄尺寸减法/clamp及IO错误退出raw恢复边界未测。benchmark用auto着色和有限替换产生plain，三组flush场景，不据存在bench宣称性能达标。动态测试进行中。


## 动态验证结果

默认和--all-features分别运行cargo test --locked -p ratatui-inline，均退出0；每种50 unit+2 differential+5 doctest通过，另5 doctest忽略。doctest中4项引用上游Terminal，不能据此扩大fork验证。日志/tmp/grow-ratatui-inline-default.log、/tmp/grow-ratatui-inline-all-features.log。禁用incremental/dev/test debug，显式本任务target；完成立即cargo clean删除1827文件467.2MiB。未运行benchmark、真实交互例子或故障注入。
