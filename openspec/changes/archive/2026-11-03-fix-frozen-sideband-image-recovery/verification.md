# 验证记录

## 核心与失败路径

`RUST_MIN_STACK=33554432 target/debug/deps/shell-fa5e9ad84e948877 image_input_recovery_tests --test-threads=1`：54 passed，0 failed。该 libtest 产物由本轮 Cargo 编译生成；同时分别执行了对应 Cargo 目标。测试内的三 backend × 用户/工具来源 × 四消费者 × 已知/未知能力矩阵覆盖 48 个组合；已有描述矩阵另覆盖 24 个组合。

核心断言包括：冻结范围、revision 和 Surface IDs 不重抓；未选中旧图片不描述，同图重复来源按保留顺序匹配；ordinary 400 不学习能力；仅一次恢复重发；描述展开仍服从原输入预算；恢复不可用不丢图；取消后不 dispatch held provider；primary durable failure 的 fail-stop 不被旁路；Memory off 在初次拒图后不再启动描述 provider。三种 backend 的原图与完整工具证据保持在主 Surface。

真实 JSONL 验证旧 U/D snapshot 在新输入或实际 compaction 后仍使用捕获时的来源，Sideband 重新加载通过。Memory-off 的六条测试核对真实 `End(Cancelled)` / `End(Completed)`、无文件和无 storage 复活。Promoted background compact 验证 1210 后恢复并提交新文本压缩事务。完整 enabled preflush + manual compaction 验证请求目的、旧来源引用与实际 Markdown。

恢复 idle clock 的新增回归运行真实 primary HTTP，再从已验证 Timeline 恢复时间；更晚的 Sideband 不改变主 API 时钟，Fresh/Seeded/无请求证据仍为0。该项包含在54个 libtest内。

## 其它回归

- `session::helpers::session_recap::tests`：34 passed。
- `session::image_describe::tests`：23 passed，包含较早 revision 的同源缓存复用和未来 revision 拒绝。
- `auxiliary_load_tests`：3 passed，实际 extension dispatch 等待 load 屏障、关闭 channel 错误、失败 checkpoint 不返回空成功。
- `idle_memory_flush_tests`：2 passed，真实命令循环、定时器、HTTP、失败重试、查询不延迟、Rewind/reset 与零 timeout。

以上共 116 个 libtest，全部通过。旧大型 actor 测试需要上述 32 MiB test thread stack；不在产品代码增加栈或重试开关。

## 真实入口

`GROW_BINARY=$PWD/target/debug/grow cargo test -p shell --test test_session_protocol_recovery auxiliary_image_process_journeys -- --ignored --test-threads=1 --nocapture`：5 passed，0 failed；cold-load clock fix的fresh final-4 binary复跑为21.27s。

真实 stdio ACP 覆盖：主图片拒图→中立描述→文字重试、restart/load、fork/rewind；四个辅助 extension；真实 read_file PNG 工具结果；跨 endpoint/backend/Authorization 的 flush_model；foreground HTTP held 时独立 flush；普通 headless prompt `/flush`。核对实际 wire 媒体、描述 Sideband raw_output、JSONL 和 Markdown，不用命名 mock expectation 推断请求用途。

模型能力域边界：同一 transport 的 reload 可直接复用已知能力；新的模型或 fork 域仍保留首次乐观发送，明确拒图后复用已确认描述。session-title 请求与图片描述在 mock 配置中使用不同 endpoint，避免相同 Auxiliary matcher 抢走响应。

`PAGER_BINARY=$PWD/target/debug/grow GROW_BINARY=$PWD/target/debug/grow cargo test -p pager --test pty_e2e_auxiliary_commands -- --ignored --test-threads=3`：3 passed，0 failed，195.29s。最新测试源构建2m27s。

覆盖 Full/Minimal 手动 `/recap`、`/flush` 和两个模式的真实 FocusLost→181秒 PTY drain→FocusGained。断言初次拒图、vision 描述、无图重试及屏幕 marker；读取 canonical trace，按 Timeline Spawn ID 关联 typed Sideband Request/Result/End，核对冻结 source refs、Result marker 和 Completed 终态。主 Surface 原图保持。展示缓存不作为 canonical authority。

最终共124个测试通过（116个libtest、5个ACP、3个PTY），没有将旧失败探针计作通过。

## 环境与边界

fresh grow 构建命令：`cargo build --locked -p cli --bin grow --no-default-features --features sandbox-enforce --offline`，通过。保留 sandbox-enforce；默认 jemalloc 的现有本地 C 生成目录缺头文件，没有修改 allocator。已清理本轮失败的 jemalloc 编译残留，没有删除其它任务共享 target。

图片 fixture 为实际编码的 32×32 PNG，符合生产尺寸要求；1×1 不应触发图片恢复。持久化测试 cwd 位于 ignored target 下的独立 TempDir，HOME/GROW_HOME 仍隔离，结束时清理。OS 临时目录的已有 ephemeral 禁写策略不用于落盘证明，其回执差异单列 backlog。
