# Verification

- 已核对 `PromptContext::render_with_renderer`：Mandatory Core 在 Audience 选择前无条件装配；primary/subagent 共用，extend/full 只影响独立角色层。
- 人工边界审查：项目内编辑和系统临时产物符合默认位置；明确指定的外部输出、任务必需的外部配置可按现有授权执行；仅因路径可写、使用绝对路径或方便而写 home/其他仓库不符合指导。
- 对比 HEAD 与修改后的 Markdown，模板控制标记和所有段落标签顺序完全相同，新增内容只在 action_safety 内。
- `openspec validate --all --strict --no-interactive`：23 passed，0 failed。
- `git diff --check`：通过。
- 本次没有 Rust 实现或模板控制流变更，未新增逐字匹配测试，也未执行 Cargo 重编译。现有 composition 测试位置已核对；静态审查不宣称证明任意模型都会遵循指导。
- 该文本随新二进制构建嵌入；已持久化的稳定 system head 保留，不改写历史。MCP 工作区与其他 change 未修改。
