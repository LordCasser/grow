## Design
Recorder::is_active 对 Pending/Open 返回 true，Disabled 返回 false。MouseScrollState 的状态查询和切换共享该判断；失败对象在重新启用时被新对象替换。保留第一次记录才打开文件。

## Validation
实际不可写目标（目录作为文件）触发 open 失败；与关闭记录器的对照状态接收相同滚动事件并比较输出，验证失败后状态为 off，下一次 toggle 返回新路径并恢复 pending 状态，未记录不创建文件。
