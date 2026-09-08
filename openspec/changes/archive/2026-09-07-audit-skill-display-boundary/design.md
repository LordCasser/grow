## Evidence
- shell/session/slash_commands.rs：InvokeSkill 返回原始 prompt_blocks 和解析引用，不将技能正文覆盖到用户文本。
- actor/turn/admission.rs：先逐 block 创建 UserMessageChunk 并按 echo_mode 持久化/发布，再调用 parse_prompt_with_skills；后者返回独立 query 与 skill_information，最终 assemble_parts_with_skills 拼接模型消息。
- prompt_parser.rs：ParsedPrompt 的 query 与 skill_information 分字段保存，拼接不负责 UI 属性反解析。
- actor/interjection.rs：model_text 添加技能信封，但 inject_synthetic_user_message 的显示输入仍为 wrapped；重放路径也保持这一分离。
- pager/acp/tracker.rs：读取 DISPLAY_TEXT 作为显示覆盖，否则沿原 chunk 文本构造 UserPromptBlock；不从 skill 的 name/args 属性恢复原文。
- tools/skills/skill.rs：extract_skill_display_text 处理旧 command-name/message/args 元素，与新 skill 属性不同；shell/helpers/session_title.rs::title_source_text 仍在生产调用它。

## Decision
未发现属性转义引入的显示反解问题。不新增 XML 解码，也不把 extract_skill_display_text 列为闲置删除项。旧格式的标题行为与新模型信封不能混为一谈。
