# Committed User image provider verification

2026-11-07，第三增量的 ChatState provider；仅覆盖已提交图片引用查询，不代表 Shell consumer 或真实长历史五秒退出已验收。动态采用前提由父侧精确栈确认：notification reconciliation worker 在 input_admission → image_describe → read_timeline → scan_journal → decode_record 上等待。动态观测与实际退出复验由父侧记录。

## Implementation

`ChatStateHandle::committed_user_image_paths().await -> Result<BTreeSet<String>, String>` 通过一个 mailbox command 查询已验证 Timeline。唯一生产 scan 位于 `Timeline::committed_user_image_paths`，遍历全部物理事件中的 `Messages.items`、`Input::Consumed.item`、`Notification::Consumed.input(Some)`，只提取 User 文本的 `image_files_envelope_paths`，返回去重 raw paths。保留 compaction/rewind 已从 Surface 移除的历史引用；不复制 User 正文或无关 events，不加入 Shell batch grammar、index、cache、DTO 或依赖。send/reply 失败返回明确 Err，正常无引用返回空集。

## Verification

固定设置：`PATH=/usr/bin:/bin:/usr/sbin:/sbin:/Users/lordcasser/.cargo/bin CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216`，工具链 `+1.93.1`，`--locked --offline`，后台任务 timeout 0。

首轮：`cargo +1.93.1 test -p chat-state --lib committed_user_image_paths --locked --offline`。编译成功，3 passed / 1 failed。失败为新增 NotificationConsumed fixture 缺少 canonical prompt_index，被 `InvalidNotification` 正确拒绝；已按现有合法 fixture 在启动 notification turn 前捕获 prompt index，未修改校验或测试预期。首轮日志：`call_939d62a30365444e9762aa49a6c93b4b.log`。

最终定向命令：`cargo +1.93.1 test -p chat-state --lib --locked --offline -- committed_user_image_paths parent_receipt_history durable_agent_message_plan_guidance_cold_retry_preserves_its_exact_consumer`。结果 7 passed / 0 failed / 0 ignored / 646 filtered out：四个新 provider 测试、两个 parent receipt retention 测试及一个相邻 cold actor query。编译 11.53 秒，测试体 0.02 秒，无 warning，exit 0。后台任务 `01a1b4a0-1fe2-7f52-b255-476e40498389`；原始日志 `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b49a-5bcd-7b72-b4a1-0421753a1f40/terminal/call_3270dbda8c86473b9d3d6c0b61f245f3.log`。已向父释放 Cargo 窗口。

四个新测试覆盖：

- 与旧完整 clone/replay 后物理 User scan 等价；三来源、多个 Text part、dedup、raw path 不按 Shell grammar 过滤、无关大正文/System/Assistant/Observation 排除，以及 cold owner 查询一致。
- compaction 和 rewind 移除当前 Surface 引用后，live/cold 均保留物理历史图片引用。
- 空会话或只有普通正文的会话返回合法空集。
- mailbox 已关闭与 reply 丢失均明确 Err。

相邻回归仅选择现有 parent receipt retention 与相应 cold actor query；未运行全 crate 或跨模块链路。五个 scoped Rust 文件 rustfmt check 与局部 diff check 已通过；`openspec validate --all --strict --no-interactive` 为 31 passed / 0 failed。

## Scope and remaining evidence

本项只修改 chat-state commands、handle、actor、Timeline 纯 scan 和 actor unit tests。开工前已有 FIFO/input-state dirty 已保留，未修改 Shell、Cargo/manifest/lock、主规范、docs 或 backlog，未归档或提交。相对开工 dirty 快照的核对依据在 `/tmp/grow-image-provider.1uQZyx/base/`。本查询的扫描耗时仍随已有事件和 User 文本增长；未宣称常数时间或端到端性能结论。Shell 消费、取消/失败不删资产、actual B no-assets/retained+orphan 场景和真实退出链路由父协调验收。
