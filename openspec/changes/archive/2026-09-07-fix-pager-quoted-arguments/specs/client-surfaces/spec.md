## ADDED Requirements

### Requirement: External pager preserves quoted argument boundaries
PAGER SHALL 使用 shell 风格引号及转义进行参数分词，并直接调用可执行程序，不引入 shell 求值。

#### Scenario: Quoted executable and argument
- **WHEN** 程序路径或参数包含通过引号或转义表示的空格
- **THEN** 保留完整参数边界，追加的会话路径仍为独立参数。

#### Scenario: Invalid command
- **WHEN** 引号未闭合或解析后的程序名为空
- **THEN** 返回配置错误，通过已有分页器失败反馈显示，不尝试拆分后的其他程序。

#### Scenario: Literal syntax and less defaults
- **WHEN** 参数包含变量、命令替换或操作符文本，或程序是 less
- **THEN** 参数不进行 shell 求值；less 的既有 ANSI 与末尾定位参数保持，其他程序不添加这些参数。
