# Change: Repair Pager recovery regressions

## Why

Backlog 的「Pager 恢复回归夹具与首条输入队列」记录了整包验证的 11 项失败。10 项 child replay 用例的磁盘夹具只有 Summary/updates，缺少现有 reader 必须读取的 Timeline，因而在到达原本需要验证的 UI 行为之前失败。第 11 项测试意外采用本机开启的 `combine_queued_prompts`，两条输入按顺序合并发送后队列自然为空；不是输入丢失。测试必须明确选择合并模式并检查真实发送 effect。

## What Changes

- 修复恢复夹具，使正常、空历史和通知先到等测试都经过可读取、可验证的 Timeline。
- 核对首条输入的接纳/发送所有权，按真实调度边界验证一次交付和 FIFO。
- 保留原有 transcript、延迟加载、幂等、内存释放与消息去重断言；记录修复前后整包结果。
- 更新 backlog 中对应条目的状态与验证链接。

## Impact

本变更只修改 Pager 测试和验证记录，不改变生产行为、持久化格式或严格恢复要求，因此 `skip_specs: true`，不虚构 delta。已有 `reconcile-response-replay-projection` 仍独立保留；本次不归档或修改其任务状态。
