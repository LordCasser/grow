# Design
私有 HookProcessGuard 持 Option<Arc<ProcessGroup>>，Drop 调 kill。子进程启动后立即创建守卫，可选 scope 登记；在 timeout 返回后，仅 Ok(Ok(Output)) 撤销守卫，其余路径显式 drop，取消则 RAII 自动 drop。不把清理放在可跳过的 await 后分支。

Unix 真实子进程回归启动后台脚本写 ready 后延迟写存活标记，确认 ready 后取消或等待超时；超过延迟仍无标记证明组已终止。组合 scope 有/无。测试脚本即使旧实现失败也会自行退出，不留下长驻进程。Windows 复用已有 ProcessGroup/Job Object，未增加平台分支。
