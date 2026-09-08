## ADDED Requirements

### Requirement: Injected skill directories include their root skill
Server/Bundled 注入目录 SHALL 使用根 SKILL.md 加递归子目录的统一发现语义，保留对应来源 scope 和规范文件去重。

#### Scenario: Injected directory is itself a skill
- **WHEN** 注入目录根及子目录均有有效 SKILL.md
- **THEN** 二者均被发现并保持 Server 或 Bundled scope。

#### Scenario: Injected path repeated
- **WHEN** 同一注入目录重复配置
- **THEN** 根技能与子技能各保留一次。
