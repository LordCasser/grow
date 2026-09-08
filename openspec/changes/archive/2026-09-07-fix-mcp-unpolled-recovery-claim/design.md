# Design
复用 RestartInFlightGuard，不增加状态实体。守卫在 spawn_local 之前创建，future 捕获它；无论是否首次 poll，丢弃 future 都释放 claim。任务运行时仍通过局部绑定持有至结束。用真实 LocalSet 在 run_until 调度完成后立即 drop 验证无首次调用、无 push、in-flight 清空，再次取得同名 claim 成功。两种 transport 共用测试矩阵。
