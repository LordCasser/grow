## Why
/copy 使用 usize 解析第一个参数，所有解析错误都回退为文件名。负数或超出 usize 范围的序号会变成复制文件目标，而不是报告无效序号，可能产生用户未打算创建的文件。

## What Changes
区分整数字面量与文件路径；带可选正负号的纯数字解析失败时返回用法错误。数字文件名可使用 ./ 前缀明确表示路径。

## Capabilities
### Modified Capabilities
- client-surfaces: 复制命令数字参数判定。

## Impact
仅 /copy 参数解析；普通路径、空参数、合法序号及其后文件路径保持现有语义。
