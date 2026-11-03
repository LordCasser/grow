# 验证

- Pager Stay 回归旧实现 2 失败，已覆盖 inactive matching view 与 active target、无关 peer/picker/draft；修复后删除相关 59 通过。额外 same-id replacement binding 回归在按 SID 清理的中间实现失败，加入原 AgentId/epoch request snapshot 后通过；成功和失败都不能退休/注销新绑定。
- 公共 ACP `session_delete_runtime_failure_facts` 在旧实现失败于 preflight 已关掉 actor；新实现验证 early refusal 保留 actor/不发 Cancel、late Shutdown 提交 Spawn 后最终拒绝且标注 runtime invalidation。测试自身在临时 GROW_HOME 子进程运行。连同原 lifecycle deletion test：2 通过。
- 同次 Shell binary：session_lifecycle 1、rename 3、explicit_close 2 通过；完整 Jsonl storage 159 通过。资格检查与最终 writer 下核对没有合成一次不可验证的快照。
- 隔离 GROW_HOME 运行最终 Pager binary 的 Dashboard 最终 678 通过、2 ignored（含 close ACK 到达前 /home 不被抢回 Dashboard）；close 最终 113、exit 44、quit 38 通过。删除 ACK effect 覆盖 false/malformed/missing reply、transport loss、true/false invalidation receipt；failure local view retirement 的最新 draft checkpoint 通过。
- `cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce` 通过；真实 stdio 首次失败于错误的 success envelope 测试假设（Delete 为 raw ACK，Close 为 ExtMethodResult）；同时发现并纠正 frontend 共用 decoder 的错误假设，按两种实际协议分别严格解析。最终 `test_stdio_session_delete_retires_runtime_and_history -- --ignored --test-threads=1`：1 通过，真实 actor 与 durable history 都消失，重复删除幂等。
- `git diff --check` 通过。没有操作用户真实 session；公共存储故障注入使用自持 writer 线程和隔离临时目录，真实 binary 验证使用 TestSandbox。

包含最终 UI retirement 修改的 composition-root build 通过；最终隔离 UI 复核 delete 59、close 113 通过。

OpenSpec 全量 strict：归档前 17、归档后 16 通过；归档校验 774 通过。
