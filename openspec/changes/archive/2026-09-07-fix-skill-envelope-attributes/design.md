## Decision
只在输出属性时调用已有 escape_xml。先按原始 name/path 去重，再转义输出；正文不转义，因为它本来允许 Markdown 和文本标记。不是对技能内容的安全过滤，也不改变技能权限。

## Verification
回归使用含双引号、尖括号和 & 的参数、名称及路径，断言属性有效转义、正文原样以及去重仍生效。先复现旧输出，再运行 skills 模块测试。

## Existing expectation
预加载 build_skill_message 存在同一问题，旧 special_chars 测试明确期待不转义。本 change 有意修改该预期，保留原始输入 fixture 并检查实体输出。escape_xml 此前为 cfg(test)，本次启用为生产私有 helper。
