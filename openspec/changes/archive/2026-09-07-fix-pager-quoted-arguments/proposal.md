## Why
PAGER 当前直接 split_whitespace，带引号的程序路径及带空格参数会被拆坏。仓库编辑器入口已有 shlex 解析，分页器应采用同样的参数分词能力，避免用户配置了合法路径却无法打开会话。

## What Changes
用已有 shlex 解析命令和参数，拒绝未闭合引号或空程序名，仍直接启动 Command；保留 less 的 ANSI 和末尾定位参数。

## Impact
仅外部分页器命令构造，不增加依赖或 shell 执行，不修改环境变量展开或管道语义。
