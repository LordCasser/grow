## 1. 调查与契约
- [x] 1.1 核对普通布局/TasksPane/状态/shortcut 的调用方和只读边界，运行用户指定 skill 查询。
- [x] 1.2 建立 proposal、design、delta 与任务，明确来源缺失和共享组件边界。

## 2. 实现
- [x] 2.1 在已交付 projection 保存共享纯 UI 状态，覆盖 task/subagent/Workflow/schedule 与 Goal/context，按历史时钟显示。
- [x] 2.2 给 TasksPane 增加最小只读展示入口，复用行和列表行为，隐藏执行动作并回归 live 调用方。
- [x] 2.3 Replay 复用正常布局、顶栏、任务栏和快捷栏；composer 槽位改成紧凑控制框，详细信息进帮助。
- [x] 2.4 接入任务列表只读键鼠导航、child future gate、局部焦点及 resize/按下失效。

## 3. 验收
- [x] 3.1 运行 projection、replay、TasksPane 及正常调用方定向测试；buffer 验证深浅主题/窄屏和无执行入口。
- [x] 3.2 真实 PTY 检查任务/child/返回、详情/搜索、暂停/resize 和资源，记录证据。
- [x] 3.3 更新用户/开发者说明和 verification，严格校验、归档并全量/归档校验。
