# 设计

主会话是图片描述/OCR/ImageProjection 唯一转换 owner；/btw 是只读消费者。SessionActor 的 image_projection_gate 使用 Tokio RwLock，主图片恢复持 write guard 覆盖辅助请求、OCR、exact-revision 重试和 durable ACK，结果通知前释放，/btw 在读取冻结 Surface 前等待 read guard。读锁只覆盖 materialize 和文字选择，不跨 provider 调用或超载 backoff；主转换不等待 SideQuestion 响应。等待监听已有 sideband_cancel，取消不影响主转换。

从同一冻结 Surface 将所有已具备描述的图片组替换成文字，用户正文、完成工具对和来源范围保留。已知纯文本 transport 若仍存在无描述图片则失败，不补做转换；未知 transport 的未转换图片保持原有乐观语义。正在运行的主转换，无论能力缓存是否命中，都先等待其确认，再消费投影。

删掉上一变更仅由 /btw 使用的 expected_revision recovery 入口；主会话保留原有 SurfaceChanged 有界重试。/btw 全程保留 prepared client/model，仅保留既有最多三次 overload-only 重试。

不将接收端生命周期债务混入此次修改；主转换以外尚未启动的未来转换不由 /btw 调度。测试必须同时证明已提交复用、转换中等待、durable ACK 前无 HTTP，以及失败不产生独立转换。
