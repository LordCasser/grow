## Why
tmux 诊断虽然已有进程期限，但两个管道线程以 read_to_end 无上限收集输出，异常程序可在期限内大量分配内存。

## What Changes
stdout/stderr 分别限制64 KiB，读取最多多一个字节识别超限；超限通知等待循环终止进程树，返回错误，不解析截断结果。

## Impact
pager-render 共享 tmux 查询，包括 doctor 与其他调用者。保留现有进程期限、正常输出解析和退出后清理窗口。
