## ADDED Requirements

### Requirement: Automatic skill discovery respects repository boundary through cwd aliases
自动技能发现 SHALL 在 cwd 和 Git root 的规范路径上执行祖先遍历与停止判断，符号链接 cwd 不得导致越过仓库边界或漏掉仓库内来源。

#### Scenario: Cwd alias outside repository
- **WHEN** cwd 是仓库外指向仓库子目录的链接，链接父目录也有 .grow
- **THEN** 发现实际子目录至仓库根的 .grow，不从链接的外部祖先发现项目技能。

#### Scenario: Local grow directory is a link
- **WHEN** 实际 cwd 的 .grow 链接到共享目录
- **THEN** 仍按本地入口赋予 Local scope，不因目标位置改成 User。
