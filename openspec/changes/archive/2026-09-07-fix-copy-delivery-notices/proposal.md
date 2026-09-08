## Why
/copy 与 /export 持久通知把 CopyDelivery::Clipboard 都显示为已复制到剪贴板，但该枚举分支也包含 Unverified OSC 52 发送；toast 已准确区分，持久记录却夸大成功。

## What Changes
共享 CopyDelivery 的持久摘要格式，保留实际后端反馈与备份路径。两个 dispatcher 复用该格式，移除重复的无条件成功文案。

## Capabilities
### Modified Capabilities
- client-surfaces: 复制/导出持久通知的交付证据。

## Impact
pager-render 反馈格式及 Pager 两个调用点，不改变复制路由、文件备份、CLI 或 toast 时长。
