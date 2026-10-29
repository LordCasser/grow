# 验证记录

2026-10-29，本地 macOS arm64、Rust 1.93.1。

- 生产 Goal 控制入口先构造失败反例：pause 后首次 request build 前压力仍为 50013，预期 27，见 `/tmp/grow-backlog-goal-red-final2.log`。
- Control 和 scope 使用同一 durable actor 命令/ACK；scope 相等时不重复重估。无效重复 Control revision 被拒绝且保留原 scope。
- 矩阵覆盖 pause/restart/definition revision/complete/Normal 切换，以及已有 cold restore → route/clear/rewind。主请求现沿既有 step_control_gate 同时保护 tag 捕获、异步图片资格查询和 build，防止捕获旧 scope 后反向覆盖新 Control。
- Goal 定向集 194 项、ChatState 602 passed / 3 ignored；最终 Shell 全量 4413 passed / 9 ignored，见 `/tmp/grow-2.5.2-shell-last.log`。完整 library feature union 首轮/较早复跑计数较大，按对应命令记录，未删除测试。
- 冷恢复初始化仍取持久化授权；route 清 provider 校准，图片缓存失效不清 Goal scope。没有第二份持久化 scope 权威，也未改变 lifetime usage。
- 开发说明同步。测试使用真实 actor 与受控持久化入口，不声称真实 provider 全链并发时序或断电实测。
