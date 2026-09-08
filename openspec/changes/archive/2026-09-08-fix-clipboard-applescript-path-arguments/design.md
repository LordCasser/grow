## Design
私有 command 构造器包装脚本主体为 on run argv/end run，通过 -e 传脚本、-- 后追加 Path 参数。脚本使用 POSIX file (item N of argv)。共享 stdin/stdout/stderr 与 detach 设置，调用者按原逻辑处理退出状态。

## Validation
真实 osascript 只运行返回参数的脚本，验证空格、双引号、反斜杠、换行、Unicode 与看似脚本的文本均保持单个参数；不读取或改写剪贴板。核对三个生产调用均通过构造器，编译附件脚本而不执行，并保留既有附件协议/私有目录测试。
