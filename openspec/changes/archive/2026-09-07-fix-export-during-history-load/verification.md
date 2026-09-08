## Regression
旧实现真实 dispatcher 回归失败：loading_replay=true 且 scrollback 仅有首条消息时，目标文件已存在，违反 partial history must not be exported 断言。

## Results
修复后 cargo test --locked --offline -p pager --lib export_ --quiet：5 passed，0 failed。新测试验证加载期间不写文件；清除加载标记并加入剩余历史后精确导出两条消息。此前原子写入、相对目录及剪贴板反馈回归继续通过。

无增量、debug=0、2 jobs、RUST_MIN_STACK=16777216；仅有既有 macOS compact unwind 警告。

## Source checks and limits
检查位于渲染和文件/剪贴板分支之前；未访问真实剪贴板。SessionLoadFailed 清除 loading_replay 并 unbind_session_id，ExportCommand::run 已拒绝无会话入口，此处为源码核对。模型实时生成仍导出调用时当前快照，不等待未来消息。不是完整网络恢复端到端测试。CLI 未重新链接。
