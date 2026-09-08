## Why
交互式导出从内存 scrollback 生成文稿，却未检查 session/load 历史回放窗口。回放尚未完成时可能将部分历史当作完整会话导出。

## What Changes
在导出 dispatcher 检查 loading_replay，加载中返回明确重试提示，不进行文件或剪贴板输出。回放完成后沿用现有导出。

## Capabilities
### Modified Capabilities
- client-surfaces: 导出等待历史加载完成。

## Impact
仅交互式完整会话导出；局部复制、CLI 持久日志导出和模型正在生成时的当前快照语义不变。
