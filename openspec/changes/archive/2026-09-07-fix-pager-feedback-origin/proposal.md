## Why
Minimal transcript 在 A 开始、切换 B 后完成时，分页器失败会写入 B 的对话。正文已有 owner，文件交接却丢失来源。

## What Changes
将待打开分页器的文件、ANSI 标记和来源绑定保存，超时重试保留该来源，完成通知写回原会话；来源失效不污染其他会话。

## Impact
Pager 请求状态、普通和 minimal 创建入口、event loop 通知及回归测试。保留既有临时文件所有权与直接进程启动。
