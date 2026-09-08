## Why
Server/Bundled 注入目录仅扫描子目录，配置与插件目录却能加载根 SKILL.md。未发现需要这种差异的约束；统一目录输入语义，避免相同技能随来源改变而漏载，并复用已有发现实现。

## What Changes
注入目录使用 find_skill_md_paths，发现根技能及子技能，保留来源 scope 与规范路径去重。

## Capabilities
### Modified Capabilities
- configuration-rules: 注入目录的根技能支持。

## Impact
agent 注入来源收集；不改变字段注入者或 reset 所有权。
