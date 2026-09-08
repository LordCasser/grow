## Evidence
apply_substitutions 依次替换 indexed/full args、skill_dir、session_id、plugin tokens。用户参数与前一轮目录值因此可能再次参与后续替换；大量正文扫描次数也随参数数量增长。

## Decision
使用已有 regex 依赖的静态 token 匹配器一次遍历原始正文，closure 返回字面值；不扫描替换结果。显式索引不限位数，溢出或缺失即空。$N 的现有 argv.len().max(1)+20 识别范围保留，因为现有契约把范围外 $100 当金额；其语法设计债务另记，不顺带统一。参数仍 split_whitespace，不增加 shell quoting 语义。上下文缺失 token 保留，只有原始参数 token 抑制后缀。

## Validation
参数包含 metadata token、indexed 参数包含其他 $N、目录包含 metadata token 均原样插入。显式大索引为空；已有金额、后缀、路径、插件、混合参数测试保持通过。
