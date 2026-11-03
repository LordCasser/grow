## Why

主 Agent 在实际使用中无法可靠查询子 Agent 状态，转而以工作区文件判断任务是否完成。用户提供的会话 `01a19a05-5281-7201-bae3-89f599512353` 中，截图对应查询的两个 child 已有 durable cancelled Ended，却因恢复后的 coordinator 缺项返回 unauthorized。代码还发现原 Turn 恢复跳过进度采样、网关交付阻塞本地快照以及查询失败标成 running 的同链路缺口。

## What Changes

- coordinator 缺项时在同一查询期限内从当前会话的 durable 来源授权并只读核对子任务终态和结果，不依赖完成缓存驻留。
- child 接入 coordinator 前从真实 signals 初始化快照，并在普通启动和原 Turn 恢复中持续采样。
- UI 进度通知仍最多一项在途，但等待网关确认不再阻止本地快照更新。
- get_task_output 的单项和多项结果用明确的观察失败状态表达不可达、超时和无效证据，不再宣称已确认运行。

## Capabilities

### Modified Capabilities

- `session-timeline`：细化子任务观察与展示交付的独立性及工具失败状态。

## Impact

仅涉及 Shell child 进度采样、get_task_output 的观察错误投影及相关回归。不改变 child 执行、持久结果、通知消费、授权或完成判定，也不新增状态服务或文件系统完成推断。
