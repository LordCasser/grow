## Context

当前链路按以下顺序工作：

1. user attachment 或 image-bearing tool result 进入 `normalize_images`。无效图片被删除，剩余图片写入 Timeline message；删除说明同时成为一个 deferred user reminder 和一个 `ImageDropped` session update。
2. request builder 从当前 Timeline Surface 克隆请求。对于尚未确认 text-only 的 canonical provider/model pair，它按契约发送原图。
3. provider 失败后，Shell 和 sampler usage settlement 共用 `is_unconditional_image_input_unsupported`。只有该分类成立，session 才记录 negative capability、生成 `ImageProjection` 并 resubmit。
4. 当前 `ImageProjection` 只能给图片附加 description。若视觉辅助和 OCR 都没有生成描述，投影函数返回 `ImageDescriptionUnavailable`，Surface 不变。

示例中的 16×16 图片已经在第 1 步正确删除；至少还有一张图片通过 normalization。`InvalidParameter: glm-5.2 is not a multimodal model` 不命中第 3 步的终态短语集合，因此第 4 步根本没有执行。更关键的是，失败不会消费或替换 Surface 上的图片：每个后续 request 都从同一个 Surface 重新组装历史，因而用户再发任何纯文本也会再次携带该图片并得到相同 400。

这里必须区分两种“保留”：Timeline message event 是不可变证据，应继续保留原图；Surface 是当前 branch 的模型可见投影，必须能够永久移除已确认会让当前会话无法采样的图片。直接改原消息或只在一次 request copy 上删图，都会破坏其中一个边界。

## Goals / Non-Goals

**Goals:**

- 将明确的 text-only 400 转换为一次经过 Timeline 校验和 durable ACK 的 Surface 修复。
- 保证修复后的立即重试、后续普通 turn、cold replay 和 compaction-derived Surface 都不会重新注入已删除图片。
- 保留视觉辅助/OCR 的较高信息量路径；只删除无法获得可用文本的 image group。
- 对 normalization drop feedback 做确定性批量聚合，同时保持模型 reminder 与 UI NOTICE 内容一致。
- 让 provider capability、Timeline evidence、Surface projection 和用户通知各有单一 authority。

**Non-Goals:**

- 不根据模型名称或静态 catalog 在首次请求前猜测其多模态能力。
- 不从 Timeline 历史证据自动复活已删除图片；用户若要重新尝试原图，应显式 rewind 或重新附图。
- 不把图片 notice 聚合扩展成通用通知 batching 框架。
- 不改变图片尺寸阈值、重编码策略、视觉辅助 prompt、OCR 引擎或无关 provider error 分类。
- 不顺带处理其他 active changes 中的 response replay、reasoning compatibility 或 compaction 通知问题。

## Decisions

### 1. 在 normalization 边界保留结构化 drop fact，最后统一渲染

`NormalizeResult` 不再把失败立即压平成 `Vec<String>`，而是先保存最小结构化事实 `{index, reason}`。`dropped_to_envelope` 在一个 batch 结束后按 `reason` 聚合：reason group 保持首次出现顺序，group 内序号保持输入顺序。单图沿用单数表达；同原因多图生成一行，例如：

`Images 1 and 3 were dropped before send: too small (16x16 = 256 px); images must have at least 512 total pixels.`

不同原因各占一行，但整个 batch 仍只产生一份 reminder、一条 `ImageDropped { notes }` 和一个 pager NOTICE block。user attachment 与 tool-result extraction 继续调用同一个 renderer，避免两套措辞漂移。wire event 保留 `Vec<String>`；结构化 fact 只存在于 normalization 内部，因此无需扩展 extension/pager schema。

**Alternative: 在 pager 合并相邻 NOTICE。** 拒绝。pager 已经只收到一次更新，重复内容在发送前就已形成；UI 层无法可靠恢复图片序号与原因结构，headless 和模型 reminder 也不会获益。

### 2. unsupported-image 分类继续共享且只接受终态能力声明

在 `sampling-types` 的共享分类器增加终态 claim `is not a multimodal model`。现有门槛保持不变：request 必须实际含图片、HTTP status 必须是 400，且 malformed/size/dimension/format/policy 等错误优先排除。claim 后只允许空白或标点，因此诸如 `is not a multimodal model for audio` 不会被误分类。

Shell failure recovery 和 sampler usage accounting 必须继续调用同一个分类器。前者决定是否改变 session capability/Surface；后者决定该 provider validation rejection 是否能按精确零 usage 闭合。不能在 Shell 单独加入 GLM 特例，否则两个状态机会再次分叉。

**Alternative: 匹配 `glm-5.2` 或 provider 的 `InvalidParameter`。** 拒绝。模型名和错误 wrapper 不稳定，真正的 authority 是“本次含图片的 400 明确声明该模型非 multimodal”。

### 3. 用现有 ImageProjection 表达两种 group disposition

不新增第二种 Timeline event。既有 `ImageProjectionEvent` 已经具备所需的 source revision、Surface identity、image fingerprint/count、tool-call provenance、durable commit 和 replay fold。扩展 `ImageShadowSource` 增加 typed `UnsupportedModel` variant：

- `Description { result_ref }`：验证 Sideband provenance，给原图片附加 reusable description；
- `LocalOcr { engine }`：验证本地 OCR provenance，给原图片附加 reusable description；
- `UnsupportedModel`：验证 replacement 恰好等于共享常量 `当前模型不支持多模态，图片已经被删除`，并将该 image group 从 materialized Surface 替换为文本。

这里由 variant 同时表达 provenance 与 apply disposition，避免引入额外 boolean 或另一套 removal event。`replacement` 仍持久化在事件中以保证 replay 字节语义稳定；validator 对 unsupported variant 强制 canonical value，阻止任意无来源文本借此进入 Surface。

对 `User` item，复用 `replace_item_images_with_text`：在第一个图片 part 的原位置插入一次标准文本，删除该 group 的其余图片，并保留原有文本顺序。对 `ToolResult`，保留非图片内容，在同一 causal item 内加入一次标准文本；与该结果配对的 assistant tool arguments、assistant carrier 和 compaction 引用继续通过已有 tool-call shadow 规则不可逆脱敏，防止资源路径再次把图片注入请求。

同一 item 的多张图片只生成一个标准 replacement，不逐图重复；每个独立 causal image group 各自保留一个 replacement，以免丢失其在会话中的位置。

### 4. 原始证据留在 Timeline，删除结果成为当前 branch 的 durable Surface

validator 在提交前仍以当前 branch transcript 检查 source、fingerprint、count 和 exact `source_revision`。apply 与 bulk replay 必须根据 disposition 使用同一纯转换：description/OCR 调用 `attach_item_image_description`，unsupported 调用 `replace_item_images_with_text`。二者都推进 replacement Surface identity，并对 derived compaction reference 和 image-producing tool path 执行相同的因果 redaction。

`record_image_projection` 继续遵循 prepare → durable Timeline commit → actor accept。只有 commit ACK 后才重置 native continuation、重算 Surface token pressure、发布 projection notification 并允许 resubmit。原始 image-bearing message event 没有被改写或删除，审计/rewind 仍可读取原始证据；正常 request assembly 只读取修复后的当前 Surface，不会因为进程重启而复活图片。

这也定义了 model switch 行为：description/OCR disposition 仍保留原图，未标记的新模型可尝试原图；unsupported removal disposition 已从当前 Surface 删除原图，切换模型不会隐式从历史证据复活它。需要再次发送时由用户显式 rewind 或重新附图。

### 5. 无可用描述时提交删除投影，而不是返回可重复的失败

primary model 明确拒图后，恢复顺序保持：配置且可用的 visual auxiliary → local OCR → unsupported removal。一个 projection event 可以同时包含三类 group：成功描述、成功 OCR 和仍未解决而被删除的 group。`ImageProjectionReport` 分开统计 `described_images` 与 `removed_images`，从而生成一条汇总 `ImageProjected` 通知，而不是逐图通知。

当所有 unresolved group 都已获得 typed removal shadow 后，不再返回 `ImageDescriptionUnavailable`。Shell 必须等待 projection ACK，然后返回 `ImageInputUnsupportedAndResubmit`。重建请求时：

- description/OCR group 由已标记 text-only pair 选择文字描述；
- removed group 在 Surface 中已经只有标准文本；
- request image count 为零，随后的纯文本 turn 也保持为零。

如果 Surface 在异步描述/OCR 期间变化，沿用现有有界三次 rebuild；每次必须重新 materialize 并重新计算所有 group。projection validation、Timeline write 或 ACK 失败时 fail closed，不发送基于内存临时删除的请求。negative capability 已持久化但 projection 未完成时，下一 turn 的 pre-sampling gate 会再次尝试 durable projection，而不是直接把原图发给已知 text-only route。

**Alternative: 只在失败请求的 copy 上删除图片后重试。** 拒绝。当前 turn 可能成功，但 canonical Surface 仍带图片；下一 turn、cold replay 或其他 request builder 会再次中毒。

**Alternative: 改写或删除原 Timeline message。** 拒绝。会破坏不可变证据、rewind 来源和 image/tool causality；Surface projection 已经是正确抽象。

### 6. 已经 poisoned 的 session 通过下一次明确拒绝原地修复

无需离线扫描或按模型名迁移。旧 session 的 Surface 若仍含图片，升级后的下一次请求可能再次收到同一 400；新分类器随后记录 capability、提交 removal projection 并自动 resubmit。成功 ACK 后该 session 的所有后续请求使用修复后的 Surface。

如果 provider 不再返回可识别的明确能力错误，则系统不能凭历史失败文本猜测并删除用户媒体；这种 session 仍需显式 rewind、重新附图或切换模型。这个限制保留了删除行为的 authority 边界。

## State and failure boundaries

```text
unknown pair + Surface(image)
        |
        | request with original image
        v
explicit image-capability 400
        |
        +--> persist negative capability
        |
        +--> materialize exact Surface revision
               |
               +--> visual description succeeds --> Description shadow
               +--> otherwise OCR succeeds ------> LocalOcr shadow
               +--> otherwise -------------------> UnsupportedModel shadow
        |
        +--> validate + durable ImageProjection ACK
               |
               +--> failure: stop, no resubmit
               v
repaired Surface(description-backed image or canonical removal text)
        |
        +--> rebuild request --> zero raw images for rejected pair
        +--> later turns/replay use the same repaired Surface
```

关键 invariant：`ImageInputUnsupportedAndResubmit` 只能出现在 durable projection ACK 之后；任意 resubmit 若仍含触发拒绝的 image group 都是实现错误。

## Risks / Trade-offs

- **删除投影有意丢失当前 Surface 的视觉语义。** → 只有 provider 已明确拒图且 description/OCR 均不可用时才使用；标准文本公开表达损失，原图仍在 Timeline 证据中。
- **切换到视觉模型不会自动复活被删除图片。** → 隐式复活会破坏 durable Surface 和 replay 一致性；显式 rewind/重新附图是可审计恢复入口。
- **一个 event 混合 attach 与 remove 两种 apply。** → disposition 是逐 group typed 且 validator 完整；同一 event/Surface revision 原子提交，比分两次 event 更能避免中间半修复状态。
- **错误短语可能因 provider 改写而继续漏判。** → 分类使用 provider-neutral 终态语义和严格 400/image-count gate；新增真实错误 fixture，并保持共享测试表方便增量扩展。
- **聚合 notice 改变英文文本快照。** → wire schema 和单图语义不变；测试固定分组、序号、顺序及 user/tool 两个入口，不依赖 pager 做二次解释。

## Migration Plan

无需持久化 schema 迁移或批量重写。新 `UnsupportedModel` variant 只由新 binary 写入；既有 description/OCR projection 继续按原语义 replay。旧的 poisoned session 在再次获得明确拒图 400 后原地生成新 projection。实现、测试、开发说明和验证完成前 change 保持 active；完成后按 OpenSpec 流程归档。回滚到不认识新 Timeline variant 的旧 binary 不受支持。
