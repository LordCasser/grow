## Why
共享剪贴板统计以 text.len() 标成 chars，中文与 emoji 被按 UTF-8 字节数高估。CLI 和交互式会话导出以及复制反馈都使用此函数，需要统一准确的单位。

## What Changes
chars 按 Unicode scalar value 计数，补全字符单位单复数；行数沿用 str::lines。没有显示宽度或字素簇计数的新依赖。

## Capabilities
### Modified Capabilities
- client-surfaces: 剪贴板文本统计。

## Impact
pager-render 共享统计 helper 的全部调用者；不改变复制路由、剪贴板内容或导出正文。
