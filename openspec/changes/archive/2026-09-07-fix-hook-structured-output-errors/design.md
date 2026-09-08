# Design
共享 is_structured_output_error(input,error) 只分类错误，不先转 Value，避免丢失重复字段等 schema 证据。错误为 serde data 或 trim 后以对象/数组起始符开头时，runner 返回 Failed；其余语法失败当作纯文本。

Prompt/Tool command 的 exit 2 始终走拒绝分支，即使 schema 或 decision 值不合法。HTTP 非2xx规则保持。Stop command 已拒绝对象错误，扩展到同一判定；Stop HTTP 不再把该类错误当 allow-stop。测试覆盖两种runner/两类gate，另以真实dispatcher验证block策略。

全测试确认 Serde 结构体反序列化支持位置数组，新增共享 parse_hook_json 在反序列化前拒绝数组，避免空数组生成默认 Stop。旧截断 deny JSON 允许测试已按新契约改为 Failed，纯文本分支仍单独验证允许。
