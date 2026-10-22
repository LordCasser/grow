## Context

process_effects 在 SendPromptBlocks / SendInterject 执行前强制 checkpoint；capture_agent 的 composer、uncertain、staged 图片均不支持文本草稿序列化。这是确定性策略冲突，日志无 ACP 发送，不能归因于 DeepSeek 或网络。

## Goals / Non-Goals

让明确携带图片的首次发送正常到达 ACP，同时保留稳定 identity、完整附件与失败恢复。不把图片编码到 256 KiB 文本草稿，不引入另一套持久附件所有权，不绕过一般 checkpoint 错误。

## Decisions

在 process_effects 的统一发送边界识别 SendPromptBlocks/SendInterject 中非空的显式 images。派发前标记现有 RAM-only identity，说明本地草稿不能恢复图片，复用既有 RAM-only checkpoint 短路和 TaskResult 的 session/binding/send identity 校验，再进入正常 request ownership transfer 与 effect executor。日志使用独立 memory_only 事件，不能把未执行的磁盘 checkpoint 记为成功。其他请求保持原 gate。

附件内容仍经 append_prompt_images 的数量、字节、文件类型校验，接纳由 Shell 负责。失败沿用原请求身份和内存快照恢复，不自动新建或重试。ReadyInputIntent 是已通过现有 gate 的请求，沿用原优先分支。

## Risks / Trade-offs

接纳前进程退出仍可能丢失图片，这是已有内存附件的边界；本次明确展示，不声称已本地保存。文本磁盘错误不能因匹配错误字符串而绕过。文本占位符不是附件。新测试必须经过 process_effects，不能只验证 append_prompt_images 或手工 ReadyInputIntent。
