## Why

macOS 的 `pbpaste -Prefer txt` 由剪贴板粘贴读取路径调用，但此前使用 `Command::output()`，子进程挂起或无限输出可能持续占用后台读取任务与内存。

## What Changes

- 为 macOS `pbpaste` 文本读取复用已有剪贴板子进程运行器，执行期限为 5 秒，stdout 与 stderr 各最多 1 MiB，并在结束时回收所属进程组。
- 超时、超限、启动或退出失败均作为文本读取错误返回；空 stdout 仍表示没有文本。
- 本 change 只覆盖 `pbpaste` 文本子进程；原生剪贴板消息及图片文件读取/解码预算仍留在独立 backlog 项。

## Capabilities

### Modified Capabilities

- `client-surfaces`: 为 macOS 剪贴板文本子进程增加执行期限、输出预算和进程组回收契约。

## Impact

- 实现：`crates/codegen/client-support/src/clipboard.rs`。
- 开发者说明：`docs/development.md`。
- 不改变其他平台的 `arboard` 文本读取、macOS 图片及原生元数据读取行为。
