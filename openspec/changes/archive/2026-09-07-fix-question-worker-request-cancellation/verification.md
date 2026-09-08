## 失败证据
提取生产 worker 启动块为 start_user_question_worker 后，保持旧逻辑运行 cancelled_question_does_not_stop_worker：第二个问题得到 RecvError，0 passed / 1 failed。两个问题在启动前排队，第一个 receiver 已丢弃，第二个等待 mock ACP 返回 Cancelled。

## 修复
通知 Hook select 的 result_tx.closed 分支由 false 改为 continue，不再经过 break。循环迭代退出释放 pending guard；shutdown 和 Hook 失败仍退出。启动函数不新增状态，只使实际循环可用测试 actor 启动。

## 范围
未读取用户会话或呈现真实问答；仅本地 mock 客户端。测试直接覆盖预关闭请求，运行中关闭共用同一 select 分支，不宣称所有 Hook 的故障生命周期已完整验证。

## 最终验证
Shell reverse_request_session_id_tests 3 项通过（含真实 worker 连续请求）；tools ask_user_question 64 项通过。命令均使用 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p 对应包 --lib 对应过滤器。rustfmt 与 git diff --check 通过。Shell 已有 linker unwind 警告不影响结果。
