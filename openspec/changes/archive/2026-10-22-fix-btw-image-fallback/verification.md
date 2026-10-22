# 验证与审查

## 已通过

- `RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo test --locked -p shell --lib side_question -- --test-threads=2`：8 passed，0 failed；新增三个生产路径回归覆盖 Messages/Chat Completions/Responses × 用户/工具图片、再次调用复用文字、普通 400、投影持久化失败、冻结后 Surface 推进。
- 同次实现编译产物 `target/debug/deps/shell-9761d0fe534f7150` 在 shell crate cwd 执行 `session::actor::tests::image_input_recovery_tests --skip side_question --test-threads=2`，设置 RUST_MIN_STACK=16777216：18 passed，0 failed。覆盖视觉辅助、OCR、混合移除、冷恢复、取消、ACK fence 和实际 text-only 错误。
- 原有 SideQuestion 测试保留完成工具证据、冻结路由、超载重试和冻结工具结果；新增成功路径检查主上下文不包含提问或回答，描述仍保留原图。
- 普通错误以完整前后上下文值比较验证无变更；持久化失败夹具会关闭 chat-state 查询，因此验证错误返回及没有第三次 provider 请求，不对已关闭 actor 的空读结果虚构原图丢失。
- `git diff --check`、`openspec validate --all --strict --no-interactive`、`openspec validate --archived --no-interactive` 通过。归档后再次执行同两项校验。

## 对抗性审查

初次审查发现旧 frozen request 的拒绝可能转换最新 Surface 的图片，已用 expected revision 读取校验及原有提交 ACK 校验阻断；该模式不重试新的 Surface。新增并发回归在 Sideband attempt ACK 被阻塞期间追加图片，释放后验证只发一次失败请求、不发 auxiliary、不转换新增图片。

准备路由的 ModelImageInputKey 与 client/model 同源；原始配置 key 不代替实际 prepared transport。转换前后共用超载 backoff，最多两次超载重试及一次明确图片拒绝后的恢复请求。每个冻结源建立独立 Sideband，失败源先终结，恢复请求重新冻结。

独立审查未发现其他阻断。SideQuestion `spawn_local` 不监听 respond_to.closed() 是既有 owner 生命周期问题，已登记 backlog，未混入本次修复。

## 限制

验证针对本地 debug 测试，不代表现有安装的 grow 已重建或替换。构建沿用共享 target，未新建独立 Cargo target 目录。编译保留已有 fetch_update 弃用及 macOS unwind linker 警告。
