## Why
CLI export --clipboard 丢弃 CopyResult，在全部剪贴板后端失败时仍报告成功且返回 Ok。用户和脚本因此无法可靠判断导出是否完成。

## What Changes
消费实际 CopyResult：失败返回错误，其他结果显示后端提供的反馈和共享统计。不改变剪贴板路由或引入文件回退。

## Capabilities
### Modified Capabilities
- client-surfaces: CLI 剪贴板导出结果反馈。

## Impact
pager/export_cmd 的 clipboard 分支；文件和 stdout 分支保持原有语义。
