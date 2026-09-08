## Why
问答响应以 label 标识选项，ID formatter find 返回首个同名选项；目前只验证问题文本唯一，重复选项会造成歧义甚至错映射 ID。

## What Changes
发送前验证每道题的 option label 唯一，不限制跨题重复。

## Capabilities
### Modified Capabilities
- tool-authorization: 问答选项唯一性。

## Impact
仅工具输入校验及测试，不更改 ACP 协议、UI 和答案格式。
