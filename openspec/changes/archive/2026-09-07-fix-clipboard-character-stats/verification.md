## Evidence
原函数 text.len() 计算 UTF-8 字节并标为 chars，且无 char 单数。全部三个生产消费者均调用共享函数。

## Regression and result
新表驱动测试在旧实现首先因 A 输出 1 chars 而失败；因此该次失败证明单复数问题，字节计数问题的修复前证据为函数体，不声称旧测试执行到了后续 Unicode 项。

修复后 clipboard_stats_count_unicode_characters：1 passed，0 failed；全部 7 个固定预期均通过，覆盖空文本、ASCII、中文、emoji、组合字符、多行和 CRLF。未访问真实剪贴板。

locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216。仅改共享文本格式，不重新编译调用方或 CLI。组合字符以 Unicode 标量计数，非字素簇或显示列宽。
