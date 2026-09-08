## Reproduction
旧实现 update_rejects_invalid_section_types 失败：skills.paths 为整数时仍返回保存成功。配置编辑采用容错解析会丢失该段原值。

## Validation
shell util::config::：228 passed，含全部配置加载和持久化回归。新真实临时文件测试覆盖 cli/models/ui/skills/session/toolset/ask_user_question 类型错误、修改闭包不执行、原字节保留，以及合法段未知字段仍正常保留。已有环境引用与缺失文件测试同时通过。

## Limits
运行时容错解析未改变；严格范围只覆盖设置保存的区域，不声称全配置所有未知/错误值均已校验。未修改真实用户配置。macOS linker 既有 __eh_frame 警告未影响结果；target 约 6.6 GiB，剩余磁盘约 73 GiB。
