# 验证

真实 fork dispatch -> source close RPC effect -> SessionCloseResolved ACK -> WorktreeSessionFailed：旧分类移除失败页并回 Welcome，指令丢失。改为 pending_fork_banner 请求身份后进入 fork draft 收敛。第一轮绿测命中复用 GROW_HOME 的历史假 cwd 草稿，原 directive 实际被正确排入队列而非丢失；因此新增 test-only draft store 注入，各失败 fixture 各持 tempdir，不修改生产默认 store 或覆写用户数据。

真实 fork dispatch -> ForkSessionReady -> SessionLoadFailed：旧实现 composer 空且 directive 清掉，红测确认。失败 replay 结束 batch/释放失败 binding 后复用相同失败处理，既不提交指令也不请求删除。

最终测试与 strict 结果见下文。

最终 failed_ 81 通过；隔离各组 draft store 的最终 fork 55、load 67、modal 17、Dashboard 679（2 ignored），delete 59、close 114、Ctrl+X 19、exit 44、quit 38 全部通过。多个过滤有重叠，不相加成唯一用例数。日志 /tmp/grow-fork-final-failures.txt 与 /tmp/grow-session-journey-final-regressions.txt。

最终 composition-root sandbox build 通过。最终 binary 的组合 Dashboard PTY 1、运行中 Ctrl+D 挂起/新进程继续 PTY 1、真实 stdio close 1 与 delete 1 均通过；没有 replay 用户原 prompt/重复执行工具的证据见退出 PTY 的文件/请求断言。日志 /tmp/grow-session-journey-{build-final,pty-final,exit-pty-final,stdio-close-final,stdio-delete-final}.txt。

最终 OpenSpec 全量 strict 归档前 18、归档后 16 项通过；含归档 strict 778 项通过。清理了本轮失败 jemalloc 编译输出与自建共享验证临时 home，保留最终 binary、验证日志及其他工作区编译缓存。
