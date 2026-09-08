# Why
stdio 重启安装客户端后 await arm_liveness_watcher，再次获取状态锁时只按名称发送当前 ToolsChanged 并返回成功。监听器等待期间同名连接可能已被配置变更移除或替换，旧任务会借用替代连接身份推送刷新并报告成功。

# What Changes
最后一次状态锁内检查 owned_clients 中的 Arc 身份；不再是刚安装的客户端则返回既有 Superseded。检查与事件发送之间无 await。

# Impact
只补齐 stdio 重启最后提交阶段的客户端身份边界，不改变 HTTP 恢复或重试策略。
