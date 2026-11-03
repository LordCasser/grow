## ADDED Requirements

### Requirement: Recap reuses acknowledged image descriptions

手动与自动 recap SHALL 在冻结输入前等待已开始的主会话图片转换及 ImageProjection durable ACK，并在冻结 Surface 请求副本上优先选择已有描述，不依赖负能力缓存。recap SHALL 保留原有 backend reasoning 选择、完整工具证据、预算裁剪和 display-only 语义。完成裁剪后，准备路由若已知拒图而实际请求仍含未描述图片，recap SHALL 在 provider 请求前走既有 unavailable/静默失败路径；未知路由保持首次乐观语义。等待 SHALL 响应 session Sideband cancellation，provider 工作 SHALL 不持图片投影锁，也 SHALL NOT 发起转换、写图片负能力或修改主 Surface。

#### Scenario: Existing descriptions after a model switch
- **WHEN** 图片已有持久确认描述，主模型切换到纯文本路由后生成手动或自动 recap
- **THEN** 各 backend 的请求使用描述而不发送对应原图，普通文本和已完成工具证据保留，主 Surface 不变。

#### Scenario: Conversion awaits acknowledgement
- **WHEN** recap 到达时主转换或投影 ACK 尚未完成
- **THEN** recap 等待后再冻结；取消或转换未能提交可用描述时，不发送已知纯文本路由的图片请求。

#### Scenario: Unresolved images are outside the retained budget
- **WHEN** 预算裁剪移除了未描述旧图片，剩余内容符合已知纯文本路由
- **THEN** recap 可以发送保留的合法请求，不因未发送的图片失败。

证据入口：`shell/src/session/actor/recap.rs` — `handle_recap`；`sideband.rs` — shared freeze；`tests/image_input_recovery_tests/auxiliary.rs` — HTTP/ACK 回归。
