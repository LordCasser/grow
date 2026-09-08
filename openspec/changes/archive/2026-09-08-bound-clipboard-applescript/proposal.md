## Why
macOS附件读取、图片读取和图片复制的AppleScript都调用无期限Command::output。挂起脚本或无限输出可持续占用工作线程和内存，临时目录也无法按正常返回释放。
## What Changes
为三条图片脚本调用提供同一私有有界runner：5秒执行时限，stdout/stderr各1 MiB，失败回收已有独立进程组，退出后管道收尾也有期限。
## Impact
client-support macOS剪贴板平台实现和隔离子进程测试；保持argv协议、native优先、临时目录所有权。pbpaste、原生AppKit调用和图像文件解码预算另行处理。
