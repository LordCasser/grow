# 观测文件模糊搜索 worker 退出

## Why

文件模糊搜索已有 Drop stop 标记与 walk cancel，但测试没有确认 daemon worker 最终离开接收循环。补充测试专用退出信号，覆盖普通空闲 worker 的退出边界。

## What Changes

仅在测试构建中增加 worker-exit 可观测状态，并增加 Drop 后 worker 最终退出的测试。此变更不改变运行时行为或已归档契约；不承诺 worker 在单次匹配或操作系统文件系统调用中的 wall-clock 退出期限。
