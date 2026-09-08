## Reproduction
临时 SKILL.md 声明超长描述和尾部 paths/disable-model-invocation，旧 parse_skill_files 仍产生技能，新回归失败。

## Validation
低磁盘配置 tools implementations::skills 100 passed，agent prompt::skills::tests 96 passed。回归确认超限 header 被跳过、无 header 长正文仍加载；已有精确边界测试保留 4096 bytes 合法闭合接受，并将 4097 bytes 闭合/超长 YAML 行更新为读取错误。

## Scope
仅在 found_opening 且超限时返回 InvalidData，底层 take 预算不变。未闭合但未超限及 YAML 错误回退未修改。CLI 未重新链接，target 9.8 GiB，可用约 68 GiB。
