## Context

两个校验点用 timeout 包裹 tokio Command.output/status。detach_command 只设置终端隔离，不设置进程生命周期；丢弃等待不是终止进程。安装器失败路径会移除候选二进制，但原进程仍可运行。

## Goals / Non-Goals

约束两个短期直接校验子进程。保留 10 秒超时、返回值及 ETXTBSY 重试。后台 run_update_subcommand 有明确独立运行契约，不参与本改动。补全生成器、任意进程树与下载取消属于其他边界。

## Decisions

在现有 Command 上设置 kill_on_drop(true)，复用 Tokio 子进程所有权，不引入全局注册表。测试使用独立临时脚本记录自身 pid，然后 exec sleep，保证被观察的是直接子进程。失败场景在断言前清理遗留进程。

## Risks / Trade-offs

终止请求后 OS 回收存在短暂异步窗口，测试有界轮询。真实进程回归限定 Unix；Windows 使用相同 Tokio API，但本机无法证明 Windows 行为。测试不调用真实安装器，也不访问网络。
