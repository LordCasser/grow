## Design
实现前复用 DoctorFixTarget 的来源信息，并核对既有 planning/apply result 的失效判断。UI 只捕获需要的屏幕/终端运行事实、工作目录及请求类型；阻塞探测与定义扫描在后台完成。结果返回后验证原会话及绑定 epoch，不把旧结果写入重绑的新会话。限制同时进行的采集，重复请求给出明确状态，避免用无限 spawn_blocking 替代 UI 阻塞。

报告结果和 fix listing/planning 复用同一份采集，不重新在 dispatcher 探测。纯 report 不触发 apply。Fix 继续生成预览并等待现有确认路径。

## Validation
通过注入被阻塞的采集函数验证 dispatcher 已返回且其他交互可继续；测试相同请求重复、来源切换/移除/重绑、采集错误与后台异常，以及 report/list/fix 的既有输出和确认路径。不能仅用 helper 单测声称 UI 全链路已改。

## Implementation decisions
复用 DoctorFixTarget 和 DoctorFixPlanned 完成路由，增加 report_only 来源标记，以便采集异常时也能区分纯报告与既有修复通知。PrepareDoctor 同时处理 report/list/fix，worker 开始后只采集一次。DoctorReportInput 持有工作目录、屏幕/Kitty/XTVersion 事实和通知设置，避免后台读取 AppView。每个 AppView 一个 Semaphore(1)，dispatcher 以 try_acquire_owned 立即接纳或提示忙；许可移入实际 blocking closure，取消等待者不会提前释放。

沿用 current_doctor_target 对初始 None→首次绑定的合法提升规则；替换、解绑重绑和 cwd 变化仍拒绝。纯报告成功和异常都使用 report_only 标记阻止失效来源通知回退到另一个 agent。既有 fix 的取消提示策略不在本次改写。
