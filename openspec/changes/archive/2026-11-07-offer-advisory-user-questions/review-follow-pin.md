# U7 · 布局坐标变化后 page-flip pin 漂移并遮住正文

## 已确认的机制

父亲核真实临时trace，证据在`/tmp/grow-ask-scroll-state-casts/advisory_reply_nav_page_down_2.rpc.log`：原正确frame offset=11、新prompt y=11、旧collapsed card实测height=2。首次正文前重建使上方card回到估计height=1，新prompt移到y=10，但follow/preserve仍true且offset保留11。该一行漂移触发六行sticky prompt，content scroll起点变为17；正文entry id=8完整、Expanded、实测height=1，区间[16,17)恰被paint排除，paint=6..6。完成后Worked仍可画，正文仍在paint之前。单击只改active，不修复；PageDown把offset钳为0并实测上方card后，原正文进入paint，provider/input/query恒3/6/2。

这次证据排除了正文未产生、entry缺失、正文折叠、paint内render失败；可行动缺口归Scrollback layout/follow pin的坐标锚定。没有采样到每次invalidation调用栈，不把触发失效的上游消费者进一步定为另一个bug。generic双击折叠是合法状态变化，不能通过禁止该动作掩盖坐标不变量。

## 最小职责与不变量

当follow-with-preserve代表把某个prompt放在视口顶部时，pin绑定的是那份内容的位置；上方估计/实测高度或结构重建改变virtual_y后，后续绘制仍须以同一内容的当前坐标定位。内容尚能放在prompt下方时，不得因旧绝对offset漂移而意外触发sticky header、排除第一行回复；真正溢出仍沿原规则消费preserve并跟随底部。

所有者是现ScrollbackState布局和follow逻辑。优先复用已有ScrollAnchor/局部capture-restore与缓存更新边界，不建第二份转录、全局pin表或问答专用导航；不强制全历史实测、不调大card估计值来碰巧挡住这一行反例。已有ScrollAnchor用于manual resize时会clamp max_offset，不能原样应用后把合法的prompt-top pin钳成0。若局部现有状态不足以跨失效保住内容身份，先说明最小内部状态的必要性；本项不增加持久字段或公共配置。

保manual阅读、常规resize、fold读意图、selection、AllTurns/SingleTurn、底部overflow与旧“pin超过total_height后退役”规则。输入、admission/Goal/工具结算、Card手势和generic sticky渲染职责不改。不得用每次卡片Reply时goto_bottom或额外PageDown替代布局修复。

## Coder Task Spec / Review Plan

一个Scrollback owner交付这一处坐标修复，预期在`pager/src/scrollback/state/{mod,layout,nav}.rs`，必要相邻同owner测试。先用现真实block/估计测量路径建立有效oldred：collapsed advisory card上方实测2→重建估计1，新的5行literal prompt已在top且preserve，append一行正文后执行真实prepare/layout+pane绘制，核prompt仍在top、正文进入paint且Buffer有marker；不能只改内部cache向量或只断言offset常量。已有ScrollTestHarness可复用。full rebuild/测量前后与新正文增高均应保持语义pin；真正overflow、past-end shrink、manual scroll/resize的旧回归继续通过。

先最小fix/newgreen，再按影响跑layout/nav与相关pane绘制定向回归；不重做整个renderer。父核最终diff和原真实trace对应的反例；局部库验收后，原PTY owner重建**无临时trace**CLI，以同mouse主链不加Tab/PageDown/其他导航通过正文可见barrier并完成newinput/自动Answered/coldreadonly。键盘与Settings最终同产物保留一次控制。任何生产修改须等诊断executor实际释放Cargo/源码，不能与其临时trace撤回重叠。
