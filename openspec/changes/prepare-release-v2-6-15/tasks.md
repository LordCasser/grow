## 1. 收尾与候选

- [x] 1.1 核对已验收功能、旧发布任务和本轮发现，单独闭合gap锚点与格式遗留。
- [x] 1.2 同步2.6.15版本和发布说明，四项恢复PTY进入core门禁。
- [x] 1.3 完成相关本地验证、文档链接和OpenSpec校验，归档必要功能change。

## 2. 正式发布

- [ ] 2.1 提交干净候选，创建annotated v2.6.15并通过validate-release.sh。
- [ ] 2.2 推送main/tag并执行官方所有平台和publish=true流程，等待同SHA门禁与构建。
- [ ] 2.3 核对公开release、十平台资产、checksum/attestation和原生version/help，记录结果并保持工作树干净。
