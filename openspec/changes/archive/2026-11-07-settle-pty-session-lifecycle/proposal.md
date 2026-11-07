# Settle PTY session lifecycle

## Why

PB-03 的实际 `PtySession::start` 丢弃 shutdown receiver，`stop` 忽略发送错误并返回成功；timeout 与 linger 没有消费方。reader EOF 修改 alive 还能让 child waiter 不等真实退出码就返回。CLI 又在 bind/register 失败或 HTTP serve 结束后丢掉唯一外层 session，没有统一结算 child。

## What Changes

先让 Session 单一 owner 管理 child 观察、stop/timeout、真实退出码与 PTY 输出完成，向调用方提供可克隆的控制/完成句柄。随后 CLI/server 消费该 owner：自然退出非 linger 关闭服务，linger 保留状态和最后屏幕；显式 stop/timeout 无论 linger 都关闭 session。HTTP stop 只有结算确认才成功，WS 终态使用同一个退出结果，启动失败清理本次 spawn 的 child，名称清理仍检查本次 port。

不承诺任意后代树回收，不从 registry PID 或名称扩张进程控制权；不更改 Pager harness 自有生命周期、不把 ptyctl 变成 shell wrapper。

## Impact

ptyctl 的 Session/PTY handle、server 和 ptyctl-cli run，以及真实 CLI/HTTP/WS 回归。行为契约分两项实现交付，接口由同一个 Session owner 提供；无需通用 supervisor 或新 registry。
