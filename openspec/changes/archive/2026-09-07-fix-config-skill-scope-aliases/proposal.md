## Why
配置技能的 scope 直接比较展开后的路径文本与 git root，而去重使用规范路径。符号链接或相对写法可能将同一来源标成不同 scope，影响 Repo/User 优先级，必须按实际配置根位置分类。

## What Changes
配置来源 scope 比较前 canonicalize 配置路径与仓库根，扫描输入与配置原文保持原样。

## Capabilities
### Modified Capabilities
- configuration-rules: 配置技能 scope 的路径身份。

## Impact
agent 配置技能分类；不改变自动发现或递归子目录的 scope 继承策略。
