## Evidence
list_skills_with_plugins 保留禁用条目并设置 enabled=false。AgentBuilder::build 把该列表交给 resolve_preloaded_skills，然后将返回内容拼入 prompt_body。原解析没有 enabled 检查，format_skills_for_injection 也只检查正文非空。

## Decision
先解析名称，再跳过禁用的匹配项，使用既有 tracing 告知跳过。不能在 find 谓词中筛掉 disabled，否则可能选中后续同名 plugin。保持 enabled 技能的声明预加载行为，不把 disable_model_invocation（禁止模型自动调用）等同于全局禁用。

## Verification
直接调用真实解析函数，覆盖禁用 native、禁用 qualified plugin、同名 enabled 后备、enabled 正常注入。用非空预加载正文保证旧实现会实际泄漏到格式化提示，而非仅构造缺失文件使测试通过。
