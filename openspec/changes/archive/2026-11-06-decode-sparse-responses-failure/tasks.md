## 1. Evidence and behavior

- [x] 1.1 核对指定两子任务原流、完整开始帧、稀疏failed及同route成功对照，明确真实服务失败和SDK错误遮蔽。
- [x] 1.2 保存旧decoder真实HTTP/SSE有效red。

## 2. Repair and acceptance

- [x] 2.1 在Responses client错误分支识别精确稀疏失败形状，复用既有error分类并保留raw/实际terminal；不伪造Response。
- [x] 2.2 验证完整链路、错误/重复字段/终态和有限恢复控制，完成Sampler回归及固定1.93.1check/fmt零warning。
- [x] 2.3 父验最终diff/证据，更新开发说明、strict与归档并提交下一版本修复；不改2.6.1或发布下一版本。
