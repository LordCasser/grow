# Verification

## 本地候选

- GitHub 最新公开正式版为 v2.6.15，远端 v2.6.16 引用为空。选用下一补丁版本 v2.6.16，workspace 版本、锁文件、changelog 一致。
- 固定 Rust 1.93.1。全仓 rustfmt、默认 CLI build、默认 CLI check、workspace 全 targets check 和 `release-dist` profile/feature 的 `-D warnings` check 均通过。全 targets check 中既有测试专属警告仍可见。
- 最终候选在隔离 Leader 中分别通过 Workflow 与 scheduler 双 ACP 客户端对照。Pager 最终定向验证 31 项通过、1 项原手动基准忽略，Shell descendant recovery 11 项通过。测试包括 1 MiB 回退、前置终态、分页乱序、子 TODO 重连和不同任务的 scheduler 通知乱序。
- 一个 GPT-6 Astra xhigh 代理做四轮独立源码复审。逐轮修正后，末轮未发现可明确复现的新 P1/P2。复核了原 resident actor 身份、分页任务归属与终态、子 TODO 重连和 scheduler 版本仲裁。
