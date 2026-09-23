## 1. Evidence and repair

- [x] 1.1 核对相关规范、replay reader、磁盘夹具、首条输入调度和 active change，记录 11 项失败根因。
- [x] 1.2 修复正常及空 child replay 夹具，保留全部原行为断言。
- [x] 1.3 按实际 SessionLoaded 发送链路修正首条输入回归，验证一次交付与 FIFO。

## 2. Verification and closure

- [x] 2.1 定向回归和 Pager 整包通过；执行改动文件格式及 diff 检查。
- [x] 2.2 记录验证与限制，更新 backlog 对应条目。
- [x] 2.3 完成归档准备：严格 OpenSpec 验证并清理本轮构建产物；归档结果另记 verification。
