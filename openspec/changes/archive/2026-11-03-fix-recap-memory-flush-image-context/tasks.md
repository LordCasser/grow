## 1. 回归与实现

- [x] 1.1 增加 recap 与 Memory Flush 真实 HTTP 回归，旧组装链路重放显示已有描述的原图仍被发送并触发 400；旧 checkout 的编译阻断与重放边界见 verification。
- [x] 1.2 拆出共享 Surface 冻结和最终图片能力验证，接入 recap；验证原 reasoning、工具证据与预算仍保留。
- [x] 1.3 接入 Memory Flush snapshot 和实际路由检查；验证旧 snapshot、窗口裁剪、ACK、失败及取消不写主图片状态。
- [x] 1.4 运行相关回归并对失败路径自查，同步开发说明与 backlog。

## 2. 规范闭环

- [x] 2.1 记录验证，运行全量 strict 校验并核对 delta 场景后归档。
- [x] 2.2 完成归档后全量及 archive 校验并记录结果。
