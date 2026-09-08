## Before
旧 dispatcher 在真实 Action { n:0 } 测试中于 n-1 发生 subtract overflow。普通 slash parser 已拒绝 0，不能将此描述为用户 /copy 0 可直接崩溃。

## Results
app::root::dispatch::tests::transcript：14 passed，0 failed。新回归验证倒序 1/2/3 的实际文件内容，非 assistant 块不参与序号，0/4/usize::MAX 不 panic 且不写文件。

源码只有目标分支调用 copy_text(false)，随后 break；移除了 Vec<String> 全量副本。不提供未测量的时间/内存改善数字。

locked/offline、无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；仅有既有 macOS compact unwind 警告。未访问真实剪贴板，未重新链接 CLI。
