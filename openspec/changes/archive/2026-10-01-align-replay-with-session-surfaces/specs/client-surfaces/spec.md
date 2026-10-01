## MODIFIED Requirements

### Requirement: Replay panel replaces live composer and status controls

Replay SHALL 保留普通会话的被动展示层次，在原 composer 槽位显示紧凑播放控制区。普通顶部历史状态、task/subagent 栏、正文与底部快捷键 SHALL 尽量复用相同组件，播放信息不得全部堆入底栏。只替换会话输入和执行动作；不提供配置、发送、停止历史任务、审批或续跑动作。

#### Scenario: Standard terminal
- **WHEN** 终端至少 100 列、24 行
- **THEN** 顶部显示当前节点和可证实的历史状态，task/subagent 列表沿用普通位置和样式；原 composer 区域显示紧凑 Replay 状态、控制、进度和历史时间，快捷键在独立底部栏只显示一次，正文占剩余空间。

#### Scenario: Narrow or short terminal
- **WHEN** 窗口缩小或含中文长标题
- **THEN** 按 cell 宽度和现有布局降级，优先保留正文、播放状态及帮助/退出；完整被省略信息在只读帮助可查，无越界或过期鼠标热区。

#### Scenario: Playback focus changes
- **WHEN** 用户打开详情、搜索、子页或播放进入 Paused/Finished
- **THEN** 快捷键与当前实际动作一致且只展示一次；无效动作禁用，状态不只靠颜色表达，详情不遮住播放控制。

#### Scenario: Reading away from the bottom
- **WHEN** 用户滚离底部或打开详情
- **THEN** 不隐式暂停也不抢回阅读位置；跟随提示或控件只改 viewport。

#### Scenario: Playback and historical state differ
- **WHEN** 播放器 Playing 而历史 Goal paused、已退出 Goal 模式或存在未完成工具
- **THEN** 历史状态继续用普通展示组件呈现，播放器状态只属于 Replay 控制区；两者不互相推导，不发起恢复/继续。

## ADDED Requirements

### Requirement: Replay task and status surfaces follow delivered history

Replay SHALL 用当前节点已交付事件构造普通被动 task/status 展示。任务列表 SHALL 复用普通行渲染、分组、排序、搜索和展开行为；已记录进度、duration、终态和状态归属 SHALL 保留。未记录的数据 SHALL 不从当前运行时补取，也不得用完整来源中的未来事实提前填充。

#### Scenario: Historical subagent is running
- **WHEN** 当前节点已交付子任务 spawn 或 progress，但尚未交付 terminal
- **THEN** 正常任务栏呈现该任务及已记录进度，点击/选择后可进入已交付的 child 过程；返回保持列表和正文阅读位置。

#### Scenario: Task finishes or mode changes
- **WHEN** subagent、后台任务、Workflow 或 schedule 的终态到达，或者 Goal/Behavior 切换、退出或 clear
- **THEN** 对应普通历史栏更新，不保留已撤销 Goal，不把模式切换当作独立任务完成，不混用父子节点状态。

#### Scenario: Inspect task without execution controls
- **WHEN** 用户在 Replay task 栏浏览、搜索、折叠或查看任务
- **THEN** 只操作本地列表或已捕获详情，停止、删除、审批、管理和当前 stdout 操作不可见且不可调用；缺失历史输出明确说明。

#### Scenario: Historical timers during compression and pause
- **WHEN** 播放加速、压缩 IDLE 或暂停
- **THEN** 任务与 Goal 已记录耗时保持原始业务意义，运行中的外推使用历史来源位置；暂停同时冻结展示，不能使用进程当前时间或压缩后播放耗时代替业务时间。
