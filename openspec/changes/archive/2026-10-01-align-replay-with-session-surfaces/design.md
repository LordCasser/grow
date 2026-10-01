## 目标与代码证据

普通 `AgentView::draw` 使用 `AgentViewLayout` 划出顶部 status、可选 TasksPane、scrollback、turn status、prompt 与 shortcuts。Replay 当前 `Replay::draw` 只有 scrollback 和最多五行 Panel；问题是布局和状态输入缺失，并非正文 renderer 不同。继续保持独立的离线协调器，不将 live App 事件处理器接入播放器。

## UI 取舍

按用户指定运行本仓库 ui-ux-pro-max 的 design-system、UX 和 style 查询。通用 design-system 给出的 conversion funnel/艳色 block 方案不适合现有 TUI，采用 style 查询中的功能性 minimalism：复用 Theme、既有间距和层次，保留文本状态及可见焦点，按需展示详细信息。TUI 不采用该 skill 的网页字体、SVG/鼠标指针和 Tailwind 建议。

页面从上到下为：正常顶部状态行（节点标题/路径及可证实的历史 Goal/Behavior、context/任务汇总）、普通 TasksPane、正文和局部详情、可证实的历史 turn 状态、原 composer 槽位的 Replay 控制框、普通 ShortcutsBar。控制框以播放状态/倍率/进度、必要播放操作与历史时间为主；IDLE/复制反馈/离底提示在有内容时出现。原长诊断行和重复操作说明移入帮助。窄小窗口按现有布局和安全宽度降级，优先保留正文、播放状态、帮助和退出；详情不能覆盖播放控件。

## 历史展示输入

已交付 ACP/Grow 事件提供上下文 token、Behavior/Goal、subagent progress/finish、后台任务及 Workflow/schedule 事实。使用已有纯 UI 状态结构喂共享组件，不读取当前任务/配置或使用最终快照提前显示未来。未记录的 model/context/输出/活动隐藏或明确未知，不伪造完整现场。任务和 Goal elapsed 使用历史来源时钟，暂停冻结；压缩轴只驱动播放，不能缩短历史业务耗时。

`TasksPane` 增加局部只读构造方式，沿用同一 row renderer、排序、分组、过滤和热区测量，省略 kill/管理动作。Live 调用方维持现有语义。Replay 只接本地列表浏览和已可导航 child/Workflow/记录详情，不接普通 pane 的执行 dispatcher。各节点各自保留任务列表阅读状态；切页清除失效鼠标按下、resize 重建命中。

共享 turn status 明确接收 input_available；普通会话为 true，Replay 为 false。历史阻塞等待保留普通活动和任务计数，但不能出现队列发送/steer 等输入提示。Finished 控制区明确写已结束，不保留继续播放动作；最后 IDLE 提示仍可在帮助中查看，历史时间恢复到控制框。

## 验证

测试覆盖事件进行中任务出现、progress 与 terminal、Goal 退出/clear、source elapsed、暂停、future gate 和 root/child 状态归属；buffer 验证与普通组件相同的行序、深浅主题、Unicode/小窗口/热区及无 kill 提示；真实 PTY 验证任务栏进入 child、返回、搜索/详情、暂停和终端恢复。回归正常 TasksPane 和已有 replay/transcript/export 测试。只清理本次专属 Cargo 产物。
