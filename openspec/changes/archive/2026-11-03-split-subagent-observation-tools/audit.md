# 对抗复核与独立债务

本次按实际公共工具与coordinator验证：subagent枚举/详情/等待不读取Terminal；command输出不访问subagent backend；没有旧混合工具实现或注册；metadata不消费结果通知；未知进度、缺runtime与损坏证据不生成执行终态。真实权限fixture涵盖root/direct parent、foreign、Workflow，等待fixture涵盖挂起、收尾、期限、分段与中断。验证命令与限制见[verification](verification.md)。

## 取消入口仍混合（独立处理）

`tools/src/implementations/grow_build/kill_task/terminal_command.rs::KillTerminalCommandTool::run`仍直接委托`KillTaskTool`，后者在`kill_task/mod.rs::run`先require Terminal、调用terminal.kill_task，NotFound后才尝试SubagentBackend.cancel。因此terminal-only名称与描述不能证明该实现只处理command；仅有subagent backend的宿主也无法走这条取消入口。两个工具还共享KillTaskAction，taxonomy不能单独表达取消对象。

本次只拆输出/观察/等待，未修改取消接口或执行控制。当前能力上限是：已有默认混合取消工具继续工作，尚无独立的无Terminal子任务取消契约。若启动取消入口拆分或需要非Terminal宿主取消child，应新建change，同时核对对象隔离、来源权限、幂等与真实进程取消收据，不能只改显示名称或再增加别名。
