# 边界审计

本次只修 Replay 的展示层次及历史状态输入，不把 live App / AgentSession 接入离线播放器。普通会话的执行管理与 Goal 恢复策略保持既有边界。

| 情况 | 核对及处理 |
| --- | --- |
| 输入框移除后仍出现发送提示 | TurnStatusArgs 增加 input_available，normal/fullscreen/minimal 为 true，Replay 为 false；阻塞等待保留活动和已交付任务数量，不显示 Enter queue/steer。 |
| 多任务与 child 导航 | 复用 TasksPane 的普通分组、自动开合、排序、搜索和 h 完成项；查看使用保存的 child identity / Workflow run ID，经过当前已交付 spawn 与直接父链 gate。 |
| 暂停/加速/IDLE 压缩 | normal widgets 接来源时钟的 FrameStamp；暂停不增加 duration，业务 elapsed 与播放轴分离，child 使用其来源偏移；Finished clamp 到该节点最后交付时间。 |
| 打断后追加输入，旧 turn 晚到 terminal | projection 原 prompt guards 和 tracker 继续归属正文；新状态栏只清当前 prompt，不因旧 terminal 隐藏新 turn。 |
| Goal/Behavior/Workflow/schedule | 与正文接受规则一致；Goal clear 不被旧 active 复活，Workflow clear/revision guard 保留，schedule delete 清栏，模式不冒充任务终态。 |
| 键鼠/resize | 列表只读，不接 App 执行 dispatcher；kill/delete/管理热区和 pending kill 不出现。Press/release 需相同 identity，切页/resize/区域变化清旧 press；帮助/详情/搜索按局部焦点返回。 |
| 信息缺失 | 未记录的 model/context/实时 stdout 不从当前执行补取。后台详情只用已捕获 stdout，缺失明确说明；播放器 Finished 不将未完成任务改为成功。 |
| 同一共享状态栏的其他调用方 | fullscreen 与 pager-minimal 显式保持 input_available=true；buttons=None 仍仅表示没有鼠标按钮，不能据此隐藏普通键盘输入语义。 |

ui-ux-pro-max 已运行 design-system、UX、style 查询；通用 conversion funnel/艳色方案与 TUI 不符，采用其一致性、渐进披露、清楚反馈和 functional minimalism 原则，复用已有 Theme 与组件。
