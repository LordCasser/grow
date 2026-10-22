## Why

四项局部优化的基准有明显收益，但旧端到端 fixture 没有现代 response admission/projection。其整体耗时不能说明现代长会话的收益与剩余瓶颈；既有父子 inclusive timers 也不能直接相加。

## What Changes

补充合法现代 Timeline、payload-free anchor 和 canonical projection 的隔离长会话 fixture，覆盖完整与部分缺失 projection；沿用真实 MvpAgent/ACP load 与 PTY 测量，核对历史顺序和持久化边界。补充分段诊断计时、首条可见历史与通知计数，记录规模、字节数及三轮结果，解释局部与整体差异并登记后续切口。

## Contract

仅增加 test-support fixture、基准和 opt-in instrumentation，不调整恢复算法、持久格式、输入或绘制语义，因此 skip_specs。实现其他性能优化不在本 change 范围。
