# 鼠标迟答后的正文可见性：有界判别

## 已取得的运行事实

无额外Tab的键盘主链与Settings同新CLI首跑2/2通过。唯一改变为正文双击的mouse完整场景在新Turn正文可见barrier失败；本轮保留了提交后原生Timeline、输入blob、provider、RPC及screen/cast，见[focused product record](verification-focused-product.md)。

`/tmp/grow-ask-focused-pty-casts/mouse-probe/advisory_reply_body_timeout.timeline.jsonl`物理第73行/domainseq62已提交assistant正文`ADVISORY_REAL_REPLY_TURN_DONE`，第78行/domainseq67为同reply Turn completed；RPC有正确literal人答、正常PromptResponse和自动Consumed/FIFO查询。三个foreground请求及旧tool结果数均正常。104个终端输出事件未出现完整marker，屏幕只有新reply prompt与Worked。输入、provider完成和持久结算已经成立，不能把此次失败描述为模型没有回答。

## 源码已核与证据限制

父与独立只读review b1ad核mouse.rs先通用handle_scrollback_click再发advisoryquery；selection.rs双击通用分支会toggle旧card。editor-open实际screen也显示旧card已折叠。Scrollback fold_selected_impl只变该entry并重建anchor，不等于新assistant被折叠。follow_new_turn可重新选择新prompt并恢复follow-with-preserve，因此旧fold也不能证明永久关闭follow。

artifact没有该reply AgentMessage entry的内部text/height/paintwindow状态。现证据不足以在tracker/candidate所有权、entry缓存及viewport/layout之间定根因，不能据此新增定性P2或把原失败改称fixture错误。当前没有生产修复。

## 下一次最小判别及停止点

保原body可见断言；只在同一mouse场景失败后做一次诊断扩展：通过现locate_screen_text找到Worked行，SGR单击取得transcript焦点；发送一次PageDown，更新/保存一帧；再一次PageDown，更新/保存一帧。不得submit、额外Reply/query、另建session或改变textselection配置，核请求/input事件数保持。非Vim裸G受lettergate抑制，不能当有效导航。

如果正文出现，记录其真实显示前提，仍保首次未自动显示的事实，由父判断操作契约和最小测试/产品调整。若两次均未出现，只收精确reply AgentMessage的entry存在性/text、cachedheight/virtual_y、visible paint range、scroll_offset/follow/preserve：有正文但不在paintwindow交viewport owner；位于窗口却不绘制交entry/render cache owner；缺entry/正文交tracker/candidate owner。任何生产改动须先得到此判别，再单独交一个修复，不把问答特性重做一遍。

## 导航实测与下一所有者判别

[唯一导航probe](verification-mouse-nav-probe.md)已实际完成：首次30秒barrier仍失败，单击Worked行并第一次PageDown后正文出现，第二帧保持；provider/input/query恒3/6/2，原panic保留，actual0pass1fail/101。未在click和PageDown之间取帧，不能拆分两动作的作用。屏幕从新prompt＋Worked变成包含原历史、新prompt、完整新正文和Worked，因此不得把原缺失简单视为正常尾部在屏幕外。

正文已在Scrollback中可达。下一诊断只采该正文在首次绘制与超时、导航前后的entry text/height/virtual_y、paintwindow/visible range、offset/follow/preserve与selection；优先复用现debug入口，若没有则临时只读trace并在完成后精确移除，不将诊断hook变成公共API或用户配置。一次实际mouse执行后冻结证据，明确落到cache、viewport或selection所有者，再交最小修复；不先以“跳过双击折叠”掩盖尚未确认的布局原因。当前没有生产行为修复，特性mouse验收仍未通过。
