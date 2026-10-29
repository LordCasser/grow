# 路径与边界矩阵

此表覆盖仓库主上下文压缩入口及共享相关设施，不把静态审阅等同端到端生产验证。

| 入口/阶段 | 代码入口 | 核对内容/证据 |
| --- | --- | --- |
| 手动 slash、RPC、mailbox | slash_exec、extensions/memory、run_loop、idle_arbitration → run_compact | 排队/lease、显式 drain、开始与失败终态；发现空转反例 |
| 模型切换 | turn/mod 两处 maybe_compact_on_model_switch | 新窗口在请求前恢复，错误传播；前序 pressure admission 回归 |
| 普通采样前阈值 | check_auto_compact_needed → pending_forced_compaction | projection 完成后跨 StepEnded fence；发现 pending manual 空转 |
| 工具结果预压缩 | check_preflight_overflow → 下一 Step | 工具结算、Goal/control fence 先行，硬窗口独立门禁 |
| provider 长度错误 | turn/sampling → CompactAndResubmit | 错误 metadata 降窗，单次无进展恢复，换 epoch 后独立恢复 |
| typed context-window stop | turn/mod 的 ModelContextWindowExceeded | 与异常恢复合流，不允许无限重新发超窗请求 |
| 异步软阈值 | background_compaction_boundary | lease/manual/Goal/flush/suppression 门禁；冻结范围；软阈值启动，硬阈值提升 |
| 后台取消与提升 | cancel_background_compaction、finish_background_compaction | owner 保留、ordinary signal 与 explicit drain；两个失败探针 |
| pre-prune | maybe_pre_prune_at | fresh skill 保护，写入前无可用计划 fallback，durable ACK 错误不可 fallback |
| 摘要准备 | prepare_compaction | Hook、物化、范围与 min source/tail、image 投影、system/输出 reserve；预算错位探针 |
| provider 与 retry | ShellCompactionSampler、generate_session_compact、shared sample_summary_with_retries | 3 次暂态重试、两次输入 70% 缩减、总 Sideband 9 次预算及 usage settlement；四类错误 stop 反例 |
| 摘要清洗 | common compaction summary | 前导 scratchpad、标签中和、最低长度；正文误删探针 |
| 提交与恢复 | commit_generated_compaction、ChatState mutations/timeline | Summary link、range identity、fresh skill、replacement ACK、control 重投影、不可反改 committed 事实 |
| UI/通知 | Pager session_notification、tracker、session finish_turn | Started/Completed/Failed/Cancelled、held prompt、pending async notice、RPC 已发布终态标记 |
| Recap | actor/recap、helpers/session_recap | 独立只读快照预算，不替换主 Surface，不与主压缩终态混淆 |
| 权限分类 | turn/sampling 的 classifier snapshot fit | 本地只读输入缩减，不提交主历史 replacement |
| Memory flush/dream | 独立 memory Sideband | 写 memory store，不写主压缩范围 |
| Fork/context recall | storage fork、actor/context_recall | fork/召回各自投影与预算，不调用主摘要 replacement；召回派生从摘要源剥离 |

## 状态边界

`freeze source → validate closed range → project/budget → admit Sideband → settle provider attempt → accept summary → revalidate authority/model/range → durable replacement → terminal → request projection/display`。

取消可发生在上述任意 await；是否已 durable replacement 决定终态，不能只按最后返回的 Result 判断。Background worker 只有生成权，前台闭合 Step 才有发布权。普通控制取消不能等待依赖同一 root inbox 的 settlement；显式 drain 需要保留 inbox 服务能力。
