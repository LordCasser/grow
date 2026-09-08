## Design
普通 dispatcher 在解析目标视图后检查 loading_replay，阻止渲染/临时文件创建并避免追加空正文提示。minimal request 在同一原 root agent 上检查 loading_replay 且 session_reload 不存在时拒绝；session_reload 存在保持现有等待路径。分别在实际正文来源处校验，避免跨子视图读取错误状态。

## Validation
三个屏幕模式逐个验证部分历史不创建文件/构建，加载完成后新请求含完整条目。保留 minimal reload 自动重建测试。
