## Context

`ModelImageInputState` 保存字符串拒绝集合。Shell 当前把 catalog alias 传给状态读写；然而 `resolve_image_description_route` 已能解析真实 auxiliary `SamplerConfig`，其 endpoint/backend/query 可能在 alias 不变时改变。反过来，多个 alias 可解析到同一实际运行时。现有 `ModelImageInputKey` 已封装 wire model、backend 和 endpoint/query fingerprint，并被 Timeline 图片 projection 使用。

## Decisions

### Use the resolved transport key for negative capability

为 `ModelImageInputKey` 增加稳定的标准 JSON `storage_key` 表示，避免另造第二种 endpoint identity。继续把该字符串存放在现有 `ModelImageInputState`；Shell 的查询和写入 API 接受 typed key，由边界统一转换为 storage key。primary request、resolved auxiliary request 与 compaction 均从各自实际 sampler config 构造 key，而非从 alias 构造。

实际 model、backend、endpoint 或 query route 任一改变即得到不同 key；完全相同的 transport key 即使来自不同 alias 也共享拒绝状态。catalog alias 继续用于标签和配置选择。已有 alias 字符串自然不匹配新 JSON key；本 change 不扫描迁移、不删除或重写该 resource 中其他值。

若 configured auxiliary resolve 到与 rejected primary 相同的 transport key，则跳过该 auxiliary call，避免对相同被拒 runtime 递归调用。若其实际 transport 已不同，即使 alias 相同也按新运行时处理。Primary 与 auxiliary 自身拒绝均记录它们各自冻结的 transport key。

### Preserve the existing fallback lifecycle

此状态只决定某个已解析 runtime 是否可以发送原图。未标记 runtime 的首次请求仍发送原图；收到已确认拒绝后，已有视觉辅助、local OCR、ImageProjection durable acknowledgement 和基于当前 Surface 的请求重建顺序保持不变。compaction 按同一 resolved key 查询；其既有 pending model identity 同时改用该 key，在 endpoint/query 变化后阻止旧请求的拒绝污染新配置。拒绝恢复传入请求冻结 key，而非在恢复时重新解析 alias。Description cache 由 source/image/prompt 身份决定，允许在 auxiliary route 变化后复用同一有效描述。

## Verification

分层验证 storage key 的稳定性和 route 字段区分、不同 alias 的同 runtime 状态共享、primary/aux 的同 runtime 防递归、same alias 的 endpoint/backend/query 改变后允许新 auxiliary route 被调用。沿用现有 direct user image、read_file/tool image、历史/切换、描述复用、OCR/aux failure、deadline/cancellation 与 projection-ack tests，补充必要的可定性回归；不为每种错误 payload 新建全流程测试。
