## ADDED Requirements

### Requirement: Explicit image submissions do not require text-only draft persistence

Pager SHALL 对携带显式图片附件的普通及 steer 输入在首次发送时保留内存请求身份与附件，并沿正常 ACP 提交路径派发，不以不支持图片的本地文本草稿 checkpoint 为前提。SHALL 展示本地图片草稿无法跨重启恢复的限制，不宣称本地已持久化。图片加载验证、服务端接纳及不明结果的原身份恢复规则继续适用；其他请求的 checkpoint 失败 SHALL NOT 被自动绕过。

#### Scenario: First image prompt submission
- **WHEN** 用户首次发送携带有效显式图片的输入
- **THEN** 正常发送包含原图片字节、正文和 prompt identity 的 ACP 请求，无需因本地草稿不支持图片而再按一次发送。

#### Scenario: First image steering submission
- **WHEN** 用户向既有前台插入携带显式图片的输入
- **THEN** 正常派发 grow/steer，保留目标 turn、interjection identity 与图片，不新增前台回合。

#### Scenario: Image request cannot be delivered
- **WHEN** 图片读取或 ACP 请求失败
- **THEN** 现有失败路径保留可用原图片和身份，允许用户审阅重试，不自动重复发送。

#### Scenario: Text checkpoint fails
- **WHEN** 无显式图片的请求发生本地 checkpoint 错误
- **THEN** 保持原错误恢复流程，本次不派发 ACP 请求；文字图片占位符不授权跳过 gate 或读取文件。
