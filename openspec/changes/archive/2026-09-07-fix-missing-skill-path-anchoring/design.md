## Decision
保留 tilde 展开与请求 cwd 拼接，使用 std::path::absolute 获得绝对路径，再执行现有 canonicalize。absolute 不要求路径存在，也不随意折叠 ..，避免改变符号链接语义。进程 cwd 不可读取时沿用原回退；错误传播需要单独设计，不在本轮改扩展接口。

## Verification
不修改进程 cwd。使用唯一不存在的相对路径，验证默认点 cwd 与相对 cwd 都返回锚定路径；已有真实目录、别名、变量管理回归继续运行。
