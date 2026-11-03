## Why

Terminal 以 leader wait 成功代替进程组完成；leader 在 TERM 后退出而后代忽略 TERM 时，reap 后又因 PGID 复用安全限制不能再次 killpg，丢失收尾责任。

## What Changes

- Unix 观察 leader 退出时使用 waitid WNOWAIT 保留身份，在 reap 前结束仍属于命令的进程组。
- 显式取消保留既有一秒 TERM grace，最终组 KILL 在 child wait 前发出；超时、正常完成和取消 fast path 共享退出边界。
- 用有界后代覆盖 early leader exit、timeout 和 normal completion，并删除临时诊断 probe。

## Scope

只处理现有命令 owner 收尾，不新增全局资源预算、不改变临时目录或模型策略。资源能力调查由 analyze-command-resource-budget-and-cleanup 交付。
