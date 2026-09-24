# 搜索 worker 退出测量

## Why

Pager 搜索 worker 的协作取消已有实现，但缺少真实 history daemon 在较大待处理 corpus 下最终退出的直接证据。现有阻塞操作测试验证了 Drop 不等待操作，也验证了 stop 后取消检查点生效；本 change 补齐 detached worker 的退出观察，并把 backlog 收窄到仍无观测或无量化的边界。

## What Changes

仅增加测试可观测性和一个大 corpus 生命周期回归用例，不改变运行时行为或已归档契约。文件系统阻塞 syscall 的退出期限以及 CPU/内存峰值不在本 change 测量范围内，仍留在 backlog。
