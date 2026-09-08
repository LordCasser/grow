## Why
/copy [N] [file] 仍将相对文件路径交给按进程 cwd 写入的 helper，活动会话目录不同于进程目录时会写到意外位置。应与会话文件操作的目录基准保持一致。

## What Changes
Dispatcher 先展开 ~，再按 agent.session.cwd 解析相对路径；继续使用原复制文件 helper，保留 Unix 强制 0600 的语义。

## Capabilities
### Modified Capabilities
- client-surfaces: 会话复制文件目录基准。

## Impact
仅 /copy 显式文件输出。剪贴板和默认备份文件路径不变，不套用 export 的保留旧权限规则。
