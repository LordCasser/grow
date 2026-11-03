## ADDED Requirements

### Requirement: Steering consumption preserves admitted text

已接纳的显式 steer 或提升为插话的 followup SHALL 在既有安全边界消费时保留用户正文，不因共享插话格式化的固定字节阈值删除中部或尾部。模型可见正文、持久消费记录与冷恢复 SHALL 保留同一文本；既有图片路径占位处理、包裹和上下文容量准入继续适用。上下文容量不足 SHALL 沿既有请求准入/压缩或明确失败边界处理，不能以无全文入口的截断冒充完整消费。

#### Scenario: Long pasted followup contains instructions at the end
- **WHEN** 已接纳的长 followup 在运行中提升为插话，正文超过 25000 UTF-8 字节且末尾有用户指导
- **THEN** 安全消费后的模型可见正文保留完整中部和尾部指导，不追加有损截断标记。

#### Scenario: Long explicit steer survives recovery
- **WHEN** 长 UTF-8 steer 经原身份持久消费后重建 Timeline
- **THEN** 恢复的用户正文仍包含完整文本，重复 drain 不产生第二次消费或丢掉尾部。
