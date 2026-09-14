## 1. 排查与方案

- [x] 1.1 核对主规范、现有 change、发送开始/结果、inbox、问答 audit、child 路由和测试，证据及限制记录在 design.md。
- [x] 1.2 加载 UX skill，形成双侧展示稿、状态语义和数据保留方案；proposal、design 及三个 capability delta 已写入本 change。
- [x] 1.3 验证本 change 和全量 OpenSpec，检查文档链接及差异，结果写入 verification.md；在提案阶段保持未归档，实施后再执行交付校验。

## 2. 发送侧工具展示

- [x] 2.1 补全三个父子工具的真实开始投影，并从 coordinator 保存目标显示身份；通过真实 send_tool_call_start → ACP → Pager 链路验证具体工具名、目标及原始输入，不手工提供正确标题代替生产路径。
- [x] 2.2 为发送消息与同 session/peer 询问增加预览和可读详情，保留 raw data；验证 Pending/InProgress/Completed/Failed 原位更新和查询成功/业务失败区分。
- [x] 2.3 实现明确的投递模式与不确定回执展示；以收到 ACK、ACK 丢失、child 关闭及安全中断场景验证不虚构已读或任务完成。

## 3. 接收与恢复

- [x] 3.1 父消息 artifact 改为原始正文，在模型投影时构造来源包装，同步内部格式/校验；验证单条限制、来源文本、模型输入不增加和重复接收幂等性。
- [x] 3.2 为父消息历史提供从 Timeline 派生的保留引用，接入即时 cleanup 与启动 sweep；验证消费后恢复、共享 hash、orphan 清理及正文损坏边界。
- [x] 3.3 从 durable Received 共用投影函数发布与恢复接收 UiNotice；验证落盘前不显示收到、投影失败不改变回执、无 updates 缓存恢复及 stable identity 去重。
- [x] 3.4 将接收通知放入实际 child 的 NoticeBlock；验证未选中 child、嵌套 child、重开视图、full/cursor load 与 live/replay 交错，不抢焦点、不增加模型输入。

## 4. 问答方向与交付验收

- [x] 4.1 为直接委派/peer audit 补齐明确方向，更新接收文案和预览；分别验证父问子、子问父、peer、重名任务以及终态重放身份。
- [x] 4.2 在 normal/minimal 下验收键盘详情、鼠标双击、选择复制、长中文/多行/图片路径、窄屏与明暗主题；检查不可变收件事实只追加一次，问答不随主 turn 提前完成。
- [x] 4.3 运行相关 shell/pager 测试和隔离 loopback 进程验证，扩展 scripts/test_local_coordination.py 或既有测试入口覆盖真实工具开始及接收行；在 verification.md 记录命令、结果与平台边界。
- [x] 4.4 更新 docs/architecture/local-coordination.md，核对每项 delta 场景的验证证据，再执行全量 strict、归档与 archive 校验；未完成的行为不能提前进入主规范。
