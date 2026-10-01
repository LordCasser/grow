## 1. 调查与契约
- [x] 1.1 只读核对用户来源、root/child/offline 恢复路径和主规范，定位历史尾部补录。
- [x] 1.2 建立最小 change，明确有序重建、cursor/去重及无执行边界。

## 2. 修复
- [x] 2.1 提取 receipt 原顺序/时间/身份，接入统一 pinned 历史规划，保留既有记录位置。
- [x] 2.2 root load、child 延迟加载和离线 transcript 使用同一有序历史，取消末尾全历史发布。
- [x] 2.3 验证 full/delta/repeated reload、rewind、缺 body/锚点的降级与 transient cursor。

## 3. 验收
- [x] 3.1 运行 shell 历史规划/load 与 pager 通信/child 回归，只读验证真实会话的 12 条回复和正文完整性。
- [x] 3.2 更新用户/开发者说明、verification，严格校验、归档及全量/归档校验。
