## 1. 实施和验证
- [x] 1.1 分离只读投影与 writer repair，通过真实 JSONL observation / writer 回归。
- [x] 1.2 覆盖 live writer 持锁且 Summary 滞后时的 full/light 读取、磁盘不变、缓存不变及冲突拒绝。
- [x] 1.3 更新开发说明和审计债务，记录测试，完成严格规范校验和归档。

