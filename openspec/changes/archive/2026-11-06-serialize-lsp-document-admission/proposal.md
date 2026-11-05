# Serialize LSP document admission

## Why

2.6.1候选cbbc751a的Core42058981176在`a_server_that_publishes_is_not_second_guessed_with_a_pull`失败（round1缺少推送报告）。检查实际链路发现：`notify_file_change`先同步入队didOpen/didChange，再调用`Documents::commit`；并行publish handler在两者之间可读旧version，`record_push`会把合法新诊断截到旧version。此窗口可以丢失本次edit的唯一判定。CI日志没有线程调度轨迹，本change不声称已证明那次失败的唯一根因。

## What Changes

只修LSP模块的文档更新入队/版本归属边界。由现Documents的同步写锁保护计划、非阻塞入队与成功提交，响应侧version读取在该边界后观察已接纳版本。失败入队不推进version/range。保留旧诊断与未打开文档界限、推送优先、pull及timeout语义。增加可控并发red/green与失败发送控制，并运行原失败入口。

## Impact

不增公共工具、依赖、schema或通用事务框架，不扩展停止的架构审查。属于已有发布门禁实际失败的必要修复。父负责契约和验收，coder仅实现此LSP owner的一处竞态。新行为delta归client-surfaces；完成后同步开发说明并归档。发布仍以最终精确提交的现官方门禁为准。
