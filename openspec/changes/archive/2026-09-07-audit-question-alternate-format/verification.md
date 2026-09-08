# 问答备用格式入口审计

## 可验证事实
- `crates/codegen/tools/src/implementations/grow_build/ask_user_question/mod.rs::AskUserQuestionInput.use_id_keyed_format` 标记 `serde(default, skip)` 与 `schemars(skip)`。JSON 反序列化不接纳 true，默认 bool=false；公开模型 schema 不暴露开关。
- 全仓检索该 snake/camel 字段及 formatter 名称，只发现字段、run 分支、formatter 定义及测试；没有生产 Rust 构造 true，也未发现其他语言入口。
- 普通 `format_accepted_tool_result` 遍历 answers 并附加 preview 和 notes。Pager build_accepted_response 支持将 selected labels 与 notes 同时返回，因此正常路径保留用户说明。
- ID formatter 的 oids 非空分支仅生成 Selected option(s)，未附加 notes；若未来程序化接入会丢弃选项旁的补充说明。oids 为空时则从 notes 生成自由文本。

## 候选与限制
已将 formatter、内部开关和只服务它的测试列入临时 R4。未删除、未改变行为。Question/QuestionOption.id 不列入本次范围，不能凭此搜索推断这些字段均无用；不推断仓库外 Rust 用户不存在。
之前同题重复 label 修复仍有独立价值：正常响应本身以 label 标识，重复 label 在普通问答也歧义；本次不会撤销该验证。

## 检查
本轮只读源码并编辑审计文档。复用此前已完成的 ask_user_question 66 项回归结果作为现状背景，不声称本轮重新运行测试或证明 ID 格式可用。没有启动 UI、请求 provider 或修改用户配置。
