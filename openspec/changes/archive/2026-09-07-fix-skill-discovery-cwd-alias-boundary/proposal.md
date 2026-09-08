## Why
自动技能发现沿输入 cwd 的字面父目录遍历，却以 Git 返回的实际仓库根作为停止条件。cwd 是符号链接时可能漏掉仓库来源并扫描仓库外目录，需要统一遍历基准的路径身份。

## What Changes
遍历与 scope 判定使用规范 cwd 和 Git root；保留 .grow 入口路径，不将入口本身的链接目标误当作来源优先级。

## Capabilities
### Modified Capabilities
- configuration-rules: 自动技能发现的仓库边界。

## Impact
agent collect_skill_config_dirs 与 list_skills_with_roots。目录去重和用户来源继续原规则。
