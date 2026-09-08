## Decision
使用 text.chars().count() 替代字节长度；计数包括换行符，保持原先统计整个文本的范围。组合字符按多个 Unicode 标量计数，不声称等于终端列宽或用户感知字素数。单个字符使用 char，其余 chars。

## Verification
表驱动固定预期覆盖空文本、ASCII、中文、emoji、组合字符与 CRLF/多行。只运行纯统计测试，不触发真实剪贴板后端。
