## Context
runner 按 command_uses_shell 决定 shell 或 source_dir 下直接路径；dedup 使用相同函数决定是否包含 source_dir。

## Decisions
仅增加 tab 与 LF；不使用 Unicode whitespace，以免改变合法路径。其他 shell 语法扩展不混入本改动。

## Validation
真实子进程测试 tab 参数和多行命令；不同 source_dir 的这些 shell 命令去重为一项。
