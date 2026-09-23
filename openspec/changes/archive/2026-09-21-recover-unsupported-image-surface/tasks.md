## 1. Normalize dropped-image feedback

- [x] 1.1 将 normalization drop outcome 保留为 `{index, reason}` 结构化事实，在共享 renderer 中按相同 reason 稳定聚合，再生成 model reminder 与 `ImageDropped.notes`。
- [x] 1.2 覆盖单图、多个同原因、多个不同原因、部分图片存活、user attachment 与 tool-result extraction；确认每个 batch 只生成一条 update/NOTICE 且相同原因只出现一次。

## 2. Classify explicit text-only failures

- [x] 2.1 在共享 `is_unconditional_image_input_unsupported` 中加入 `is not a multimodal model` 终态 claim，保留 400、实际 image count 及格式/尺寸/策略排除门槛。
- [x] 2.2 增加精确 GLM 错误 fixture、provider/model 前缀、尾随标点和非终态相似文本反例；确认 Shell recovery 与 sampler usage settlement 使用同一分类结果。

## 3. Add durable unsupported-image Surface projection

- [x] 3.1 扩展 `ImageShadowSource`、validator 和 canonical replacement 常量，表示 description、local OCR 与 unsupported removal 三种逐 group disposition。
- [x] 3.2 统一 live apply 与 bulk replay：unsupported group 在原 causal item/首图片位置留下单个 `当前模型不支持多模态，图片已经被删除`，移除 raw image，并同步处理 compaction reference、image-producing tool call 与 response carrier redaction。
- [x] 3.3 更新 projection report、continuation reset、token pressure 和 storage consistency validation；证明原 Timeline message/payload 保留而当前 Surface identity 前进。

## 4. Close the sampling recovery loop

- [x] 4.1 将视觉辅助、OCR 和 unsupported removal 组装为同一个 exact-revision projection；只在 durable ACK 后返回 `ImageInputUnsupportedAndResubmit`，commit/ACK 失败时 fail closed。
- [x] 4.2 增加集成测试：首次请求含图并返回精确 GLM 400、辅助模型未配置且 OCR 失败、resubmit 为零图片并含标准文本、随后纯文本 turn 成功且不再携带旧图片。
- [x] 4.3 覆盖部分 group 描述成功/部分删除、image-bearing tool result、并发 SurfaceChanged rebuild、negative-cache pre-sampling gate、cold replay 以及 projection persistence failure；确认没有 in-memory-only lossy retry。

## 5. Documentation and verification

- [x] 5.1 更新相关开发者说明，明确不可变 Timeline evidence、durable Surface removal、model-switch/rewind 边界和 grouped notice ownership。
- [x] 5.2 运行受影响 sampling-types、chat-state、shell、pager 定向测试，必要 package check、changed-file rustfmt、`git diff --check` 与 `openspec validate --all --strict --no-interactive`，把最终证据写入 `verification.md`。
- [x] 5.3 独立复核 classifier false-positive、Timeline replay 和 request image-count invariant；所有 Rust 验证完成后检查 `target/` 并执行 `cargo clean`，归档 change 后验证 active 与 archived specs。
