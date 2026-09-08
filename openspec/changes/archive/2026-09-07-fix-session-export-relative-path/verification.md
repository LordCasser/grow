## Regression
旧实现实际 dispatcher 回归失败：在不同 session cwd 中未生成预期相对目标文件。旧进程目录位置由 tempdir_in 管理，失败后自动清理。

测试夹具先修正了两处自身问题：cwd 属于 agent.session；tempdir_in 返回规范绝对路径，因此从其 basename 构造真实相对参数。这些失败不作为产品 bug 证据。之后未改生产实现的测试才在预期落盘位置断言失败。

## Results
修复后 app::root::dispatch::tests::transcript：11 passed，0 failed。回归验证 session cwd 下的真实文件、Markdown 精确内容、进程 cwd 下未写入以及绝对目标不改写。

locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；仅有既有 macOS compact unwind 警告。

## Limits
测试没有改变进程 cwd 或 HOME，没有访问真实剪贴板。未单独重测 ~ 展开后端；代码保留同一 shellexpand 调用。同步文件写入及非原子覆盖风险尚未处理。CLI 未重新链接。
