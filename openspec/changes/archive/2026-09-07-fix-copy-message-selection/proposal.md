## Why
/copy N 在选择前将全部 assistant 消息格式化为 String，复制最新回复也会处理整段历史。Dispatcher 还假定 N 非零，直接 n-1 索引；虽然 slash 解析器拒绝 0，内部 Action 边界仍可能 panic。

## What Changes
逆序遍历到第 N 条即停止，只格式化该消息；保留不足数量的反馈，并在 dispatcher 拒绝零索引。

## Capabilities
### Modified Capabilities
- client-surfaces: assistant 消息选择成本与边界。

## Impact
仅 CopyAssistantMessage 的选择阶段；文件、剪贴板及 Markdown 渲染语义不变。
