# 同步 Plan resume PTY 的输入就绪边界

## Why

2.5.0 同提交的 Core push 门禁两次在 deferred Plan resume 场景失败。场景看到第一段恢复历史就发送 /view-plan，但历史先于 SessionLoaded 可见，恢复期间普通输入按现有契约被忽略。

## What Changes

仅在恢复进程启用既有 log instrumentation，等待本进程 pager.replay.finalize 完成标记后再输入。保留 Plan 内容、审批与恰好一次实施断言及原有时间预算。不改产品行为、持久化、输入放行边界或 CI 选择；skip_specs。
