## Findings
format::render_update 在初始 parse、逐条 parse/update 后，对完整 updated 再次 parse_block；不属于 owned_item_prefix 或与外层 namespace 冲突的条目由最终解析拒绝，不能据 validate_request 未直接检查前缀就认定会写坏文件。

mod::plan 对n个请求分别调用item_state，每次解析原文；render_update 初始一次、每条一次、最终一次，共2n+2次完整解析（不含预览managed_block额外解析）。没有测量墙钟时间，不将计数直接换算为用户可见卡顿。

typed_inspection_and_item_updates_share_one_validated_parse 只断言原文、非托管正文、新旧条目及替换保留，不统计解析次数，名称不成立。改名为 typed_inspection_and_item_updates_preserve_unmanaged_content，所有断言保留。R17等待用户确认，不借改名删除字段或断言。

## Validation
重命名后的同一测试1 passed（0.04s），所有原断言保留；全量规范16项严格通过，归档后再次校验。
