## Verification
全仓 Rust allowed_tools/allowed-tools 引用检索并阅读 parser、SkillInfo、RPC、Pager 详情和技能消息构造；检查权限目录与 CLI disallowed_tools 区别。无行为修改，不运行新测试，也不以此前 parser 测试证明权限 enforcement。未检查仓外客户端消费者。
