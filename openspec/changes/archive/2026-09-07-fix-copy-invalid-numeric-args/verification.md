## Regression
旧 parser 的新测试在 -1 上失败：参数被当作文件路径，而不是错误。该失败来自纯参数解析，没有实际创建文件。

## Results
slash::commands::copy::tests：8 passed，0 failed。新增矩阵覆盖 0/-1/+0/超过 usize::MAX 的无符号及带 +/- 形式，各自带或不带尾随文件参数；另验证 ./123、./-1、123.txt、带空格路径、+1、2 out.txt。

locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；仅有既有 macOS compact unwind 警告。未操作用户文件或剪贴板，未重新链接 CLI。
