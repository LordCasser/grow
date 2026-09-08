## Reproduction
旧 earlier_setting_failure_keeps_newer_selection 实际失败：None -> Minimal -> Fullscreen 后第一次保存失败，UI 变 None，而非 Some(fullscreen)。

## Final validation
- 设置 dispatcher：117 passed。
- 完整 app::root::dispatch::：879 passed，包含上面的 117 项，不相加。
- 新回归覆盖旧失败不覆盖最新值、连续失败恢复初始基线、先成功后失败恢复已成功值、ABA 合并到最新值、不同 key 独立、递归 reset 仅登记一次、结束后可再次保存。
- 五个旧测试补入前一次保存完成事件，维持其连续独立写入/重置的原断言；两个非法 theme 值兼容测试直接调用原 rollback arm，避免伪造与已接纳请求基线不符的结果。未删除这些断言。

## Limits
结果通过真实 dispatcher 与 Effect/TaskResult 验证，不启动真实磁盘故障或 ACP 服务。一次只发一个同 key effect 的接纳边界负责顺序，实际 settings writer 沿用既有函数。未测试任务强制取消、退出时队列排空或进程崩溃。macOS linker 既有 __eh_frame 警告未影响最终结果；未修改真实用户配置。
