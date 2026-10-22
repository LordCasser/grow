# Design

以已归档原始诊断为基线，用 cargo clippy --fix 应用编译器机器可用建议。逐文件复核自动修改，尤其短路求值、临时值 drop 次序、测试断言。TCP fixture 读取必须获得至少一个请求字节，不错误要求一次 read 填满缓冲区；unix mode 使用等值八进制 0o0。运行 fmt、workspace all-targets check、默认 CLI check，重扫 Clippy；对测试 fixture 修复运行既有定向测试。此 change 不宣称剩余 Clippy 通过。

自动建议失效时只作人工等价修正，不启用 broken-code。测试辅助代码的 cfg 必须继续只作用于原项；光标、零宽字符和投影 tail 分支通过定向回归核对。剩余类型布局/资源策略不混入本项。
