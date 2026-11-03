# 缩短命令输出工具名称

## Why

command和subagent观察已分离，terminal一词不再提供必要的对象区分。按用户要求缩短工具名，同时保持command域明确。

## What Changes

唯一工具ID与默认模型名称从get_terminal_command_output改为get_command_output，Rust工具类型同步为GetCommandOutputTool。更新注册、preset、提示、UI识别、测试和当前开发说明；不保留旧名别名。输入、输出、权限、等待和command-only实现不变，不改三个subagent工具。

## Impact

影响session-timeline的模型工具名称契约，以及tools、agent、shell、workspace与Pager中的现有引用。历史归档与持久化记录不改写。
