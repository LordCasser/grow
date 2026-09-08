## Reproduction
旧解析将错误开关值当 false，实际文件发现拒绝回归失败。

## Final validation
低磁盘配置 tools implementations::skills 103 passed，agent prompt::skills::tests 96 passed。两字段错误矩阵覆盖 yes、数字、null、列表和映射；保留 YAML 布尔、字符串 true/false 和缺省默认断言。旧字段转换测试使用合法 false，原 yes/数字输入继续作为拒绝场景测试。

## Limits
只修改元数据解析，不改变运行时权限机制；未声明其他字段校验完成。CLI 未重新链接，target 9.8 GiB，可用约 68 GiB。
