## Design
以一个待处理请求替换互相依赖的 path/ANSI 字段，把原 root AgentId、实际目标 AgentId（root 或 child）与请求时 session id 放在同一生命周期内。普通入口沿用现有可见 agent/权限优先解析；minimal 在 finish 时使用其构建 owner。不要从完成时的 active_view 反推来源。

临时文件继续由 TempPath 拥有。挂起超时重排完整请求。通知定位原 root/child，并校验 session id；匹配时写入原会话。原会话当前不可见时也发布应用级简短反馈，不能向其他会话正文写入。来源已移除或重绑时仅给应用级反馈，保留失败可见性。源码核对发现 minimal 不绘制普通 toast，因此该模式在恢复后的终端 live 区之前直接插入独立通知行，不挂到任何会话 ScrollbackState；其他模式沿用应用 toast。成功退出仍静默。Editor 的既有通知路径保持独立。

## Validation
覆盖 root A 请求后切到 B、child 来源切回 root、原来源被移除或重绑、pending 请求被替换、挂起超时重试保留来源及 TempPath 生命周期。核对现有普通/minimal transcript 与 event-loop 测试并执行相关 suite。实现前确认测试构造方式与所有 pending 字段消费者，避免只验证辅助函数。

## Scope
不修改 minimal 子 agent 渲染语义，不引入后台快照写入或新的请求队列，不改 PAGER 分词、终端挂起协议或安装二进制。
