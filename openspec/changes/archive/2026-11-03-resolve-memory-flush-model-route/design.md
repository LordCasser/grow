# 设计

有显式 flush_model 时调用现有 resolve_aux_sampler_config，再沿既有辅助模型流程 stamp session-local attribution/auth 和 retry 配置。endpoint/backend/model/认证均来自该 resolved route。无效别名明确失败，不回退主模型。无 override 继续使用一次主配置准备快照。

跨 provider mock 验证使用不同 endpoint 与 backend，主 visual 保留原图，纯文本 flush 第一次明确拒图后通过配置的描述路由恢复；记忆落盘和 Sideband 来源保持原冻结坐标。
