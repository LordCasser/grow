# 验证记录

## 定向验证

使用与 `align-replay-with-session-surfaces/verification.md` 相同的 CLI 包 debug=0 命令行 profile（不修改仓库配置）：

| 验证 | 结果 |
| --- | --- |
| `cargo test --locked -p shell --lib communication_history` | 9 passed，1 ignored：缺失 receipts 原位置/时间、物理行保留、缺正文/锚点、synthetic cursor full replace、stream/offline 同序且无源写入、Incoming 三阶段补缺、同 ID 不同 peer、错误 cache seq 不成为锚点。 |
| `cargo test --locked -p shell --lib session::storage::tests` | 53 passed：rewind、repeated rewind、cursor、response reconciliation、stream 与 typed replay parity。 |
| `cargo test --locked -p shell --lib session::storage::transcript` | 26 passed：离线 tree/observer 恢复与固定来源边界。 |
| `cargo test --locked -p shell --lib parent_receipt_is_transient_and_history_restores_without_delivery` | 1 passed：真实 actor receipt 的 live 展示仍 transient/no cursor；被消费后只读 history 能恢复正文，不再投递 gateway 或产生模型输入。 |
| `cargo test --locked -p pager --lib app::subagent::tests` | 42 passed：正常 child receipt/inherited updates、已有 live notice 去重、懒加载。 |
| `cargo test --locked -p pager --lib export_cmd::tests` | 6 passed，export 沿相同离线规划。 |

隔离 fixture 的 Incoming 恢复后 Timeline 仍只有原 5 个事件（2 Input + 3 Inquiry Observation），没有新 Notification/Request/Hook，updates/timeline 原字节一致。cursor fixture 确认 synthesized history 强制 full replace，meta 没有 eventId。

## 用户指定真实来源

只读检查 `01a0ee47-1ad4-70f0-9c87-d0319a70d814`（ScriptOS）：最新被审计输入后的 canonical assistant/reasoning 和工具事实都存在；12 个 Agent replies 的通信请求均可定位。使用环境变量开启 ignored readonly 检查，不直接 `grow resume` 该活动 Goal 来源：

```sh
GROW_SOURCE_SESSION_ID=01a0ee47-1ad4-70f0-9c87-d0319a70d814 \
GROW_HOME=/Users/lordcasser/.grow \
cargo test --locked -p shell --lib \
  session::storage::communication_history::tests::source_session_receipts_replay_near_their_reply_anchors_without_writes \
  -- --ignored --test-threads=1
```

该场景通过环境变量 gate，无源 ID 写入默认测试 fixture。命令应使用上方相同的 profile override，避免重新生成默认 debug 残留。

- 第一轮固定截点：history_rows=12305，12 receipt 位置 1853–10492，最后 response 在 12300；updates/timeline 全文件 hash 一致。
- 后续来源正常追加：history_rows=12512，12 receipt 位置仍为 1853–10492，最后 response 在 12504；每条均在关联 tool 后、下一锚点前。中间一次整文件 hash 断言因外部追加失败，改为检查原前缀字节不变且未截断，允许合法 append；最终 1 passed。
- 这 12 条不再成批位于最新输入之后/历史末尾，后续正文没有被通知替代。

真实来源只验证恢复规划与读边界，没有启动 provider、工具、Hooks 或 Goal。普通 resume 继续沿原执行策略，内部 isReplay 仅作 history load 标记。该修复没有声称恢复上次 viewport 像素位置。

## 场景核对

Delta 的缺缓存/已有位置/child/reconnect/缺正文锚点/Incoming 阶段分别有上述纯 planner、fixture、normal child 和真实源证据；原缓存行不做墙钟排序。大历史插入扫描按 increasing Timeline seq 线性前进，避免 per-receipt 全表扫描；输入/工具/通信身份均从 Timeline 核对。

最终 strict/archive 校验结果见本记录末尾。

## OpenSpec 完成检查

归档前 `openspec validate --all --strict --no-interactive`：16/16 passed。两项 change 归档后主规范 strict：14/14 passed。首次 archived 校验仅因本次归档收尾任务尚未勾选而报告两项未完成；完成核对后再校验：570/570 passed，所有 tasks 已完成。`git diff --check` 通过。

最终格式检查：19 个修改/新增 Rust 文件执行 rustfmt --check（skip_children=true）通过；仅检查本次文件，未重排无关既有测试。验收完成后执行 cargo clean -p shell -p pager -p pager-minimal -p cli -p ptyctl-cli，清理 3.8 GiB；当时磁盘剩余约 22 GiB。验证阶段未替换已安装的 grow。
