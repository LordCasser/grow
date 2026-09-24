## Context

`resolve_skill_path` 先展开 `~`，将相对路径和 cwd 拼接，再尝试 `std::path::absolute` 与 `dunce::canonicalize`。目标不存在时故意保留锚定路径和 `..`，因为词法折叠会跨越符号链接边界。add/remove 在解析后更新配置，删除函数当前不报告是否匹配。

## Decision

在 add/remove 请求入口验证 cwd：将其规范化为现有目录，并验证目录可读取；失败返回 `invalid_params`，不调用配置更新。随后仍以原 cwd 和原路径调用既有解析器，保留当前原文/别名行为。缺失目标不引入 inode 历史数据库：保存的是当次解析得到的路径表达式；每个后续请求重新按当时的文件系统解析。旧符号链接删除后，别名无法证明原目标，不能因此删掉某个规范目标。`remove_skill_path` 返回是否实际删除配置项，响应据此区分已移除与未匹配，保持同一响应结构。

## Verification

用不存在或非目录 cwd 验证 add/remove 在保存前失败；用缺失中间组件和 `..` 验证锚定表达式保留；用符号链接删除后别名与保存规范路径分别移除，核对配置项及 no-match 反馈。运行 shell 技能扩展定向测试与 OpenSpec 严格校验。
