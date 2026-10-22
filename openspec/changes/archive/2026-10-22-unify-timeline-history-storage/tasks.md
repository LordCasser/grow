## Tasks

已按逐场景映射完成实施与验证，最终结果、失败修正、测量限制和构建清理见 [verification](verification.md)。以下通过范围是所列专项回归，不冒称全仓库所有测试。

- [x] 逐族列出 durable updates 的来源、唯一内容、顺序、身份和所有读者，保存变更前 golden history；补全 Workflow/Control expectation 等明确缺口，未覆盖族不得通过删除测试消失。
- [x] 新 schema 实现最小 typed 展示引用/anchor 和原字节 checksum；统一 admission 提交，保留 artifact publication、exact identity、preview budget、ACK 不明与 fsync 边界。
- [x] 统一 cold/resident/child/transcript/fork/import/export projector，替换 cursor/frontier，删除 updates writer、repair 与派生索引；不引入兼容双写或新的持久 cache。
- [x] 验证 anchor/独立事实交错、quarantine/discard、compaction/rewind、半个 response cursor、resident 竞争、headless 身份、权限/Hook/图片恢复/Workflow/Control 历史、来源与脱敏，以及 full/minimal TUI 完整 load barrier。
- [x] 验证非法/损坏正文、断尾、缺 artifact、同身份异 payload、同步失败/lost ACK、fork/import 执行隔离、挂起树与 Ctrl+D 后恢复；对比相同业务历史的 release 前后时间、峰值内存、全目录大小与写入/fsync 成本。
- [x] 更新架构/开发者说明，记录逐场景证据、实际删除的路径和未获收益；完成 OpenSpec strict 校验与本项专用构建清理，随后按流程归档。
