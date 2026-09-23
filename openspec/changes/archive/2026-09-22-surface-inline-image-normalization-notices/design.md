## Context

`handle_prompt` 先经 `parse_prompt_with_skills` 得到 context/query/raw_images。raw_images 调用 `normalize_images_with_notices(&mut context, ...)`。之后 `extract_base64_images(query)` 返回清理后的 query 和额外图片；当前代码直接 normalize 后仅取 images，遗漏 dropped/compressed/fallback 的统一证据。

现有 helper 已拥有 worker admission、分组后的删除 reminder、压缩/fallback reminder 和对应 UI updates。组装后的 context/query 随当前用户消息进入既有 Timeline/Surface 流程；无需新队列或单独持久化结果。

## Decision

query extraction 保持当前位置和语义，仅将第二次 normalization 交给现有 helper，并继续把清理后的 query 与 surviving images 交给原路径。通知依然属于本次 Session；分组、索引、英文文案直接复用现有 normalization 结果。权限文本继续来自 cleaned query，不把 runtime reminder 当成用户授权。

所有仍有效的图片都按原顺序保留。无内嵌图或完全正常图不产生多余通知。附件与内嵌图仍分批处理，避免为局部反馈缺口改变整个 prompt parser 或图片 identity。

## Verification

通过实际 `handle_prompt` 入口断言模型可见用户消息与 gateway 通知：多个同原因坏图汇总一次、压缩图保留且有压缩说明、正常图保持且无损耗通知、混合输入中 surviving image 数量正确。使用本地测试夹具，不依赖远程模型。修复前先证明回归失败，修复后运行相关图片/admission 回归；Rust target 复用本轮隔离目录并统一清理。
