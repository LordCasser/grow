## MODIFIED Requirements

### Requirement: Skill source aliases cannot increase precedence
自动与显式配置根发现的技能 SHALL 保留根来源边界：规范目标仍在规范配置根内的文件继承根 scope；递归链接目标离开该根时，按规范目标位置分类并选择根 scope 与目标 scope 中优先级较低者（Local < Repo < User），未知位置按 User。User `.grow` 根与 home 的路径别名 SHALL 按规范身份识别；显式选择的 `.grow` 根别名仍保留其入口 scope。发现路径原文 SHALL 保持用于来源展示与加载。

#### Scenario: Repo link reaches User root through a Local alias
- **WHEN** cwd `.grow` aliases the User root and a Repo config root has a descendant link to a skill in that User root
- **THEN** the linked skill remains User scope and is not promoted to Repo

#### Scenario: Home root uses an alias spelling
- **WHEN** User `.grow` is reached through a home-directory alias whose canonical root is the same
- **THEN** the source remains User even if the alias spelling is lexically inside a repository

#### Scenario: Repository root links to an external skill
- **WHEN** a skill below a Repo config root is reached through a descendant symlink whose canonical target is outside known project roots
- **THEN** the skill is classified as User rather than Repo

#### Scenario: User root links into a repository
- **WHEN** a skill below a User config root is reached through a descendant symlink into a repository
- **THEN** the skill remains User and is not promoted to Repo

#### Scenario: Explicit Local root alias retains entry scope
- **WHEN** cwd `.grow` itself is a symlink to a shared root and a skill target remains inside that resolved root
- **THEN** the skill remains Local

#### Scenario: Descendant link from aliased Local root leaves its root
- **WHEN** a skill below an explicitly selected Local `.grow` root alias is reached through a child symlink outside the resolved root
- **THEN** the skill is classified by its canonical target, capped at Local precedence, and cannot retain Local scope for an external User target
