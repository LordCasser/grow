## 1. 契约与实现

- [x] 1.1 为异步完成通知增加“下一次 request projection 后发布”的 delta spec。
- [x] 1.2 在 compaction runtime 中暂存异步完成元数据，并在普通请求构建成功后恰好一次发布。
- [x] 1.3 待通知期间禁止重入另一轮后台压缩，保留同步/手动路径原语义。

## 2. 测试与验证

- [x] 2.1 更新异步压缩集成场景，验证提交后无通知、request projection 后数值一致及无重复。
- [x] 2.2 运行受影响测试、格式/差异检查和 OpenSpec strict 校验，记录结果。
- [x] 2.3 归档 change，执行归档后校验，并按用户要求运行 `cargo clean`。
