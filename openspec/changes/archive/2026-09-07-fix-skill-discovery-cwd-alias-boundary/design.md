## Evidence
collect_skill_config_dirs 用 dir == root 停止 cwd 向上遍历，cwd 未解析而 root 来自 git2。目录本身的 canonicalize 仅用于去重，未用于遍历停止。

## Decision
在两入口先 canonicalize cwd，Git workdir 也统一解析，失败保留原路径。只规范父级位置，不将 .grow 链接实体替换成目标，确保从本地入口发现仍为 Local。

## Verification
临时仓库含本地与仓库 .grow，外部祖先也有 .grow；从指向仓库子目录的外部链接发现，不能包含外部祖先且必须包含仓库根。本地 .grow 指向共享目录时继续 Local。
