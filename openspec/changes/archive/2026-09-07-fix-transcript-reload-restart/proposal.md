## Why
分帧 transcript 在重连暂存正文时继续跳过旧 EntryId，可能只输出已渲染前缀。简单暂停不能覆盖完整回放后的 ID 替换及两帧间完成的重连，必须在重连入口记录构建失效。

## What Changes
重连入口标记原 agent 的在途构建需要重建并释放前缀。reload 期间暂停取出构建，结束后从最终 scrollback 重新取 ID；期间新请求同样等待。

## Impact
仅 minimal 在途构建生命周期；普通标签切换、已生成文件和分页器行为不变。
