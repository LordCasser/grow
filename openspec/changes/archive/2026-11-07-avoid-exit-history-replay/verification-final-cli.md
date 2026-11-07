# Final default CLI acceptance

## Artifact and entry

最终默认生产 check/build task `01a1b4e5-2894-7420-9fec-433d7c48cade` 在固定 Rust 1.93.1、locked/offline、JOBS2、INCREMENTAL0、STACK16MiB 下 exit0。产物 `target/debug/grow`，SHA256 `9f7585d55f2a387bbebfd68cd16dbe59c5dfac0658df6fe8372a346fb8551472`，版本输出 `grow 2.6.2 (8ab486a6e) [alpha]`。工作区仍 dirty；该提交号只是构建基线，不代表修复已提交，候选源清单归发布验证记录维护。

父直接运行独立 PTY driver `/tmp/grow-complex-cli-acceptance.py`，task `01a1b4e7-b510-7ba2-a1c1-ebe3b468cee2` exit0。复用已成功挂起的自有 synthetic source HOME `/tmp/grow-complex-exit-fixed-96t0c57f`；从不加载用户原会话。原始 source 305,653,152 bytes、2,494 events，256×64KiB 工具结果、17 次 repair、31 个完整 compaction。

第一次和第二次冷恢复都在观察到 SessionLoaded 与 `prompt_history_applied` 后立即发送 `0404`；没有额外预热、sample、Force 或提高五秒 runtime 期限。

| Run | Load to actual ready | Runtime confirmed | Send to actual wait | Exit / PTY EOF |
| --- | --- | --- | --- | --- |
| first | 53,810.21ms | 125ms | 236.87ms | 0 / true |
| cold resume | 53,119.82ms | 107ms | 240.73ms | 0 / true |

两轮各自新增 checkpoint/tree 引用匹配，第二次恢复关联前次挂起 operation。未发现模型请求，先前全部 canonical prefix 和最初 source prefix SHA `54618f647d2c835c7e50dad4177c25d2eab690afbe0217a77a0f8e613e574465` 均保持。完整报告 `/tmp/grow-262-final-cli-9f7585d5/acceptance.json`，对应每轮 metadata、trace 和 raw PTY 位于同目录。该 driver 同时要求 actual wait、EOF 和持久引用，不以画面消失或 command channel close 代替退出。

## Earlier failures and limits retained

- 原现场完整 validated owner 控制为旧 copy+replay 25,050ms，实际新查询 3.916ms，191 组 Spawned/Ended facts 相同。另一个旧 helper 控制约3.9–4.0s，未误报为五秒旧红。
- 同复杂 source 原 public ACP 在5,002ms unconfirmed；图片清理 worker 的真实栈确认整史 decode。修复后同 ACP 123ms suspended、actor/writer retirement、prefix 与 cold validation 完整通过。
- 首次 CLI driver 仅等 replay finalize，prompt history 仍在启动，UI 明确拒绝退出；它不算退出超时旧红。随后按真实 input-ready 事件校正。
- 一个较早2.6.2产物的真实就绪轮出现 filesystem watcher 退休4,747ms，runtime4,845ms已确认，但进程6,019ms仍未退出。该 OS/native Drop 慢段未采到动态栈。后续两轮同旧产物约225ms，新最终产物两轮如上；不据此宣称所有 OS native 调用都有固定时延。证据 `/tmp/grow-complex-cli-acceptance-ready-8cf671e4/` 保留，通用 fsnotify 退役抖动单列 backlog，本项未混入 backend 重写。

本项关闭的是已证实的长历史重复 clone/replay 与图片整史维护瓶颈，并保留五秒确认及持久、真实退休要求。最终候选验证没有修改或截断任何 source 历史。Windows 未执行本 PTY 产品场景，不将 macOS 测量外推为全平台时延保证。
