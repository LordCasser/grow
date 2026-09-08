## Why
普通与 minimal 分页器会话文件通过 std::fs::write 使用默认权限创建，仅正常分页结束后删除。挂起失败、排队请求替换及应用退出可能留下包含会话正文的文件。

## What Changes
使用共享私有临时文件创建函数；pending 请求持有 TempPath，正常结束、错误、替换及正常应用释放时由所有权清理。挂起超时重试保留所有权。

## Impact
Pager Markdown 与 minimal ANSI 两个入口和事件循环；不改变渲染与分页器参数，不承诺进程崩溃后的清理。
