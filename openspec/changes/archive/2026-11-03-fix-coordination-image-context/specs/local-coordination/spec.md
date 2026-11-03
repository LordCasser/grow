## ADDED Requirements

### Requirement: Inquiry snapshots reuse durable image descriptions

父子与 peer 协调问答 SHALL 从同一准备配置固定接收方的 client、请求模型和图片能力路由。冻结上下文前 SHALL 等待已开始的主会话图片转换及其 ImageProjection durable ACK，并在请求副本上优先选择所有已确认的图片描述，不依赖负能力缓存。等待 SHALL 响应询问取消和会话 Sideband 取消，provider 请求期间 SHALL 释放图片投影锁。协调问答 SHALL NOT 启动图片转换、写入图片负能力或因拒绝而修改主 Surface；已知纯文本路由仍有未转换图片时 SHALL 在 provider 请求前失败，未知路由的未转换图片保持首次乐观发送语义。

#### Scenario: Switch from a multimodal model to a text model
- **WHEN** 主会话由多模态切换为纯文本路由，原图与已确认描述仍保留在 Surface，子 Agent 或 peer 向其询问
- **THEN** 无工具问答请求使用接收方准备的模型和文字描述，不发送已描述原图，不再次调用辅助模型，原图事实保持。

#### Scenario: Conversion awaits durable acknowledgement
- **WHEN** 协调询问到达时主会话图片转换或 ImageProjection durable ACK 尚未完成
- **THEN** 协调请求等待，ACK 完成后才冻结并发送文字分支，HTTP 等待期间不持有图片投影锁。

#### Scenario: Inquiry is cancelled while waiting
- **WHEN** 等待主会话图片转换时询问取消或会话 Sideband 关闭
- **THEN** 询问以取消终结，不发送 provider 请求，也不启动新的图片转换。

#### Scenario: Text-only route lacks a committed description
- **WHEN** 准备路由已知拒绝图片，但冻结上下文仍含未描述图片，或主转换无法提交
- **THEN** 询问在 provider 请求前失败，主 Surface 与负能力事实保持原状。

#### Scenario: Unknown transport rejects unresolved images
- **WHEN** 未标记路由的未转换图片首次请求被 provider 拒绝
- **THEN** 询问记录失败，不启动共享图片恢复、不修改能力状态或主 Surface。

证据入口：`crates/codegen/shell/src/session/actor/coordination.rs` — `handle_coordination_inquiry`；`sideband.rs` — 共用冻结入口；`tests/image_input_recovery_tests.rs` — 真实 HTTP 与 durable ACK 夹具。
