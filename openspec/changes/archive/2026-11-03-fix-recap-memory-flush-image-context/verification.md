## 改动与不变量

recap 与 Memory Flush 使用 shared `freeze_sideband_surface`：等待已开始的主图片转换及投影 ACK，在副本选择已确认描述。recap 随后保留既有 reasoning/预算处理；flush 在捕获 snapshot 时选择文字，继续使用原 input_ref、IDs、revision，执行时不重抓 live Surface。已知纯文本路由仅在最终选中窗口仍有图片时失败。

Memory Flush 的 client、显式 model、图片 capability key 由同一 prepared config 生成；已有 flush_model override 先作用于该配置。没有新增跨 provider 路由解析、转换 owner、恢复循环或持久化格式。未知路由且没有描述的图片仍保留首次乐观发送；图片拒绝不会由辅助调用写负能力或改主 Surface。

## 验证

`cargo test --locked -p shell --lib image_input_recovery_tests::auxiliary:: -- --nocapture`：**9 passed**。

- 两个图片矩阵各覆盖 Messages、Chat Completions、Responses × 用户/工具图片 × 有/无负能力缓存，共 24 个组合。检查实际 HTTP 无原图、有描述、有工具正文、无工具定义，主 Surface 和能力状态不变。flush 同时验证真实临时目录中的记忆文件内容。
- `previous_auxiliary_assembly_replays_the_text_only_content_type_rejection` 复用未改动的真实 recap/flush builders 重放旧组装链路：对带已有描述但仍含原图的请求注入截图同款 HTTP 400 `1210: messages.content.type ... ['text']`，实际捕获 1 张图片；再运行各自修复入口，捕获 0 张图片并成功。
- ACK/失败用例同时覆盖两条入口：等待主辅助 attempt ACK、等待 ImageProjection ACK、失败不发送已知纯文本图片、不新起转换；辅助 provider attempt 前能重新获取图片写锁。flush 在等待时切换 live endpoint/model，实际请求仍使用原准备配置。
- 取消测试确认等待可退出，flush lock 被释放，无 provider 请求。
- 未描述图片测试区分未知路由的真实 HTTP 拒绝与已知纯文本路由请求前失败，两者都不写记忆、不写图片状态；另测窗口外未描述旧图不阻止最终合法请求。
- flush override 测试确认 capability key 属于实际 flush model，不继承另一个主模型的拒绝状态。
- `memory_flush_keeps_pre_compaction_sources_and_writes_durable_text` 通过真实 JsonlStorageAdapter 保存主 Timeline/Sideband；先捕获工具图片 snapshot，再正常追加新输入，执行仍只用旧证据。核对原 input range、source revision、selected context IDs、实际 HTTP 和记忆文件，再执行严格 Sideband ledger 校验及 session 重载。

复用上述 Cargo 构建的 `target/debug/deps/shell-fa5e9ad84e948877`，设置 `RUST_MIN_STACK=33554432`、`--test-threads=4`，运行相关现有过滤组：

| 过滤组 | 结果 |
| --- | --- |
| `session::actor::tests::recap_display_only_tests` | 20 passed |
| `side_question` | 12 passed |
| `session::actor::coordination::tests` | 12 passed |
| `image_input_recovery_tests::coordination_` | 5 passed |
| `session::helpers::session_recap::tests` | 34 passed |
| `session::helpers::memory_flush::tests` | 26 passed |

这些过滤组可能重叠，不将其简单累加为独立用例数。已覆盖 recap 自动门禁/展示/取消与配置快照、portable 工具配对、辅助窗口预算、协调与侧问的旧行为。

`rustfmt --check --edition 2024 --config skip_children=true` 对 recap、sideband、新增辅助测试通过；memory_dream 和既有图片夹具只保持本次片段格式，未对用户既有内容做全文件格式化。任务相关 `git diff --check` 通过。全量 OpenSpec strict 校验：**21 passed、0 failed**。

## 发现的前置缺陷与失败记录

最初尝试运行旧树回归时，工作区另一个未完成的 `agent/subagent/mod.rs` 测试缺少 `acp` 引入，导致编译失败；该处随后由并行工作修正，本 change 未修改它。旧请求的实际 400 重放证据使用上述真实 builders 与 HTTP 路径取得，不声称完成了整个旧 checkout 的 red run。

实际文件回归首次推进历史使用了仅允许 turn 前的 ContextRebuild 夹具，被契约拒绝；改用正常 user append 后保持了“live context 已推进”的原始断言。随后发现工具图片投影生成的合法 tool-call/carrier Surface IDs 在重载时被 `owns_surface_id` 拒绝，报 `InvalidSurfaceSelection`。没有删除该失败场景或放宽来源校验；独立 change `fix-image-projection-sideband-source-coordinates` 修正所有权边界后，同一文件回归通过。该前置修复另有旧谓词失败、新谓词及 shadows-only 边界通过的定向证明。

所有 HTTP 请求均发往 loopback mock，没有付费服务调用或线上模型验收；未执行无关完整 workspace/CLI 多进程回归。构建复用共享 target，没有创建新 target；其他并行 Cargo 工作持续使用该目录，因此不删除共享缓存。临时探针和本次日志在记录结果后清理。

## 归档校验

两个 change 已分别通过 `openspec archive <change> --yes` 归档：`fix-image-projection-sideband-source-coordinates` 合入 session-timeline，`fix-recap-memory-flush-image-context` 合入 model-sampling 和 memory-search。归档后 `openspec validate --all --strict --no-interactive` 为 **19 passed、0 failed**。archive 首次校验只报告本 change 待登记的归档后验证任务；登记完成后最终 `openspec validate --archived --no-interactive` 为 **809 passed、0 failed**。相关源码、规范、开发说明及 backlog 的 `git diff --check` 通过。
