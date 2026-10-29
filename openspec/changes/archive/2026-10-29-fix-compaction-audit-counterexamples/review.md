# 对抗性复查

- 取消：普通路径只 cancel 原 token，不 join 未结束 worker；ready worker 仍走已有终态记录。真实阻塞 provider 在取消返回后释放，再走发布边界，验证不能提交。
- 排队：保持 pending manual 原文；普通 turn 以 ControlEnded 经既有终态/Step fence 结束，idle 仲裁才消费手动请求。不引入抢占正在执行工具的路径。
- 选区：先扫描原始 history 中完整边界与工具配对，再从最长候选开始使用真实 projection 估价；不以 raw reasoning 预算提前 break。不假定 portable 投影成本单调，避免重复 tool IDs 等改变投影的反例。通常只估一次完整来源，原 ID target 不变。
- 预算：system/输出/指令/误差 reserve 仍独立；选区估价扣除已经预留的 system，不双计。minimum source 按实际来源检查；verbatim 保留 reasoning 的 backend 仍计入 reasoning，portable 模式则排除。缩水重试复用同一估价函数和冻结 Surface。
- 输出：provider attempt 先准确结算 usage，再由摘要 adapter 判断可否作为 replacement；共享生成器只能得到已经验收的正文。失败不是删除历史，且保留现有重试预算。Responses EOF 不再补造成功。
- 清理：最低长度只能防退化，不能证明保留事实；正文引用标签必须保留前文，明确 scratchpad 仍可移除。

独立 Plan handoff owner 查询、跨多个选区连续摘要策略与审计登记的未证实组合缺口未混入此次实现。

## 全库回归暴露的收尾

旧 Goal budget fixture 断言后台 owner 必须仍在 slot；真实取消后 worker 可在后续 invalidation 中被回收，这个断言不属于生命周期契约。测试改为验证 原 Goal 为 BudgetLimited 且无法再准入、usage settlement 完成、明确 drain 后 Failed 且无 Completed；该 fixture 在 terminal event 前阻塞，usage 已到达，因此取消仍结算已知的 15 tokens；没有把取消当作零费用。保留限时结算及禁止新 attempt 断言。

两条既有 formatter 用例要求识别粗体 Analysis 的括号说明和同行 scratchpad。仅接受明确 Markdown 标题形式，未恢复“所有非数字正文均可删除”的旧判断，并补充 Analysis of… 自然语言正文的反例。

未丢弃任何失败测试。内部超窗错误改用既有分类器已识别的 canonical code；通过原断言验证走输入缩减路径，不修改分类器扩大匹配范围。
