## Design

`platform::get_text` 已在 macOS 模块中构造 `pbpaste -Prefer txt` 命令。将该命令交给同模块的 `run_clipboard_script`，沿用已有 5 秒 `SCRIPT_TIMEOUT` 与每条输出流 1 MiB `SCRIPT_OUTPUT_LIMIT`。此运行器已通过独立进程组拥有子进程及后代、限制管道收尾时间，并在超时或输出超限时终止进程组。

随后仍通过 `checked_command_stdout` 处理退出状态，保留现有空输出与 UTF-8 替换解码语义。限制以 stdout 的 1 MiB 上限约束返回文本输入大小；这不限制 `String` 转换的额外分配，也不声称覆盖 native/AppKit 或图像路径。

不新增第二个进程运行器或单独预算常量，避免重复维护已经由图片脚本路径验证的生命周期机制。
