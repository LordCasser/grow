## Why

R6 后恢复仍经过完整 `Value` 中转、response 集合临时复制、typed → JSON → typed 回放。先移除这些确定的重复工作，为后续存储改造建立可比较的 release 基线。

## What Changes

- 在现有 JSONL reader / Timeline decoder 内直接读取版本与类型，再解码领域 payload，避免构造完整 `Value` 树。
- 同次 projection plan 复用 branch 选择结果与借用/shared 正文；使用现有 `Arc<str>`，不新建全局索引或缓存服务。
- typed projection 直接进入既有通知转发逻辑；最终 ACP wire 编码保留。

## Impact

涉及 chat-state Timeline 查询/解码、Shell JSONL reader、response projector 与 replay 转发。不改磁盘编码、projection digest、领域执行边界或客户端协议。实施前对抗性核对发现：旧的中间 Value 会吞掉 payload 重复键；直接 typed 解码会拒绝重复的 typed 字段。现有 writer 不产生该歧义编码，故保留更简单的 typed decoder，并用 `session-timeline` delta 明确这一拒绝边界，不再标记 skip_specs。

不删除 updates、不新增检查点/压缩/对象格式、不分页、不提前开放输入，不改 resident delta 调度。当前仅提案，任务未实施。
