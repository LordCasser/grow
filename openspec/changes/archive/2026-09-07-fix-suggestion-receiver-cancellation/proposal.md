## Why
AI Suggest 2s 与 Prompt Suggest 45s 超时只丢弃 oneshot receiver；run_loop 后台任务不观察关闭，继续生成且持有 activity。

## What Changes
两个 suggestion 任务监听 receiver 关闭，优先取消并 drop 生成 future，未开始时不 poll。

## Capabilities
### Modified Capabilities
- model-sampling: suggestion 接收端生命周期。

## Impact
run_loop 两个分支与共享交付 helper；已有 Sideband Drop 负责取消终态。
