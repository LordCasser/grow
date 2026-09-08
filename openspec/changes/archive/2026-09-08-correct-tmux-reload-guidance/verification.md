## Evidence
官方手册明确tmux在server启动时加载配置一次，后续可用source-file。旧文字在fix预览caveat、reload_instruction（含无法安全显示路径的fallback）、普通诊断tmux_reload_note、truecolor说明及用户指南均给出重连替代建议。

现在统一明确显式重载，truecolor保留随后reattach步骤。引用与转义实现不变，现有doctor_format_tests三条快照和reload_instruction路径预期同步。

## Checks
4个修改的Rust文件经rustfmt --emit stdout --config skip_children=true语法解析成功，不改写无关格式。全诊断目录不存在旧or detach and reattach和Detach and reattach to activate提示；指南规范链接存在。全量规范16项严格通过。

仅修改文字，没有执行Cargo测试，没有实机tmux验证（当前无tmux可执行文件）；不将同步快照预期宣称为测试已运行。沿用上一批cargo clean后的空构建缓存。tmux自定义配置目标识别已独立登记，不在此更改路径策略。
