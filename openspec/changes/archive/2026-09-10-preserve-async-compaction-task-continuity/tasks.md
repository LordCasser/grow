## 1. 证据与设计

- [x] 1.1 核对指定 session 的 Timeline、精确请求 body 和响应尾帧，记录已证实事实与推断边界。
- [x] 1.2 核对压缩、continuation、turn 准入/结束架构及既有 change，完成 proposal/delta/design。

## 2. 实现与回归

- [x] 2.1 为摘要追加历史范围说明，在实际获准的异步压缩下一 Step 前持久化单次 synthetic 续接提示。
- [x] 2.2 验证真实请求顺序、正常 terminal、后台失败/取消/完成边界和持久化失败；保留 native reset 与采样恢复原语义。组合故障注入的覆盖边界见 verification.md。
- [x] 2.3 更新开发者说明并登记独立 portable 历史债务。

## 3. 验证与归档

- [x] 3.1 运行相关测试及编译检查，记录结果和限制。
- [x] 3.2 对照全部场景，执行全量 OpenSpec 校验、归档和归档后校验。
