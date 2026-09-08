# Why
直接执行的相对命令按 source_dir 解析。不同来源目录中的同名脚本实际是不同文件，但去重只比较命令文本，会静默丢弃其中一个检查。shell 命令在 workspace cwd 执行，不应简单把所有 source_dir 都加入键。

# What Changes
直接相对命令的去重键包含 source_dir；shell 与绝对路径保持原规则。抽取共享 shell 路由判定，防止去重与执行分支漂移。

# Impact
真正重复的命令仍 first-wins，不改变环境/timeout 优先级或相对命令执行 cwd。
