## Reproduction
旧实现 preloading_respects_disabled_skills_without_name_fallback 失败：结果包含 blocked、plugin-blocked、allowed，期望仅 allowed。测试使用预加载非空正文，避免依赖文件不存在引起的偶然排除。

## Validation
agent prompt::skills：90 passed。新增测试断言 native 与 qualified plugin 禁用项被排除，不选后备同名技能，正常技能正文进入格式化提示且不包含禁用或后备正文。

## Limits
验证范围是实际预加载解析和提示词格式化函数，不声称完整模型请求或 UI 已跑通。已安装 grow 未更新。当前 target 约 4.3 GiB，磁盘剩余约 76 GiB，未在编译期间执行清理。
