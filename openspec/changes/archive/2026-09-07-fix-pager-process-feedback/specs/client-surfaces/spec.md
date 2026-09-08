## ADDED Requirements

### Requirement: External pager failures remain visible after terminal restoration
外部分页器 SHALL 保留启动和退出结果，在终端恢复后通过当前屏幕模式可见的通知反馈失败。

#### Scenario: Pager cannot start
- **WHEN** 分页器可执行程序不存在或启动返回操作系统错误
- **THEN** 界面恢复后显示启动失败及原因，不把失败静默当作正常结束。

#### Scenario: Pager exits unsuccessfully
- **WHEN** 分页器返回非零退出状态或被信号终止
- **THEN** 界面恢复后显示失败状态，临时文件仍按请求所有权清理。

#### Scenario: Successful exit or pending handoff
- **WHEN** 分页器成功退出或终端挂起仍等待重试
- **THEN** 成功退出不新增失败提示；未启动的重试请求保持既有等待反馈，不报告进程失败。
