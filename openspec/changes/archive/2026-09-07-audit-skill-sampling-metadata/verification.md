## Verification
核对解析、SkillInfo/RPC 字段、技能提示构造、admission/interjection 和采样入口。未找到技能 model/effort 覆盖消费者；结论限定仓内实现，不断言仓外客户端行为。

低磁盘配置运行 shell `build_skill_information_for_refs_loads_and_wraps`：1 passed。该测试验证正文展开及包装，不用于证明采样覆盖或覆盖缺失；消费者结论来自源码。编译存在既有 macOS linker unwind 警告。

R10 候选保留，未删除。无行为修改，CLI 尚未重新链接。target 9.8 GiB，磁盘可用约 68 GiB。
