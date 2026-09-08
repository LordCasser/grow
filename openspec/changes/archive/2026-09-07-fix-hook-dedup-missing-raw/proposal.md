# Why
HookSpec 支持程序化及 serde 构造，raw 展示字段可缺失。registry_from_specs_deduped 将缺失 raw 替换为空串，两个实际 command/url 不同的有效规格因此碰撞，后者静默消失。

# What Changes
raw 存在仍使用 raw；缺失时使用实际 command/url。命令键采用 OsString 保持路径字节身份，不将非 UTF8 路径有损转换。

# Impact
只修复缺失展示信息的去重，不改 first-wins、matcher/on_failure 或已存在 raw 的覆盖语义。
