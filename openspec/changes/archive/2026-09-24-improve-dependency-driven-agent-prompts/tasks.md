## 1. Design artifacts

- [x] 1.1 核对当前 prompt、生产装配、工具语义、测试和相邻 change，将逐处保留/替换分析与基线写入 `prompt-analysis.md`、`verification.md`；通过路径、符号和引用复核确认。
- [x] 1.2 完成 proposal、design、英文 prompt 草案和 local-coordination delta；逐项核对需求与候选文本，确认不引入新 runtime 能力。
- [x] 1.3 完成验证矩阵与实施边界，执行单 change 和全量 strict OpenSpec 校验及文档检查；在 `verification.md` 保存实际结果，只勾选已验证的设计工作。

## 2. Audience and role implementation

- [x] 2.1 实施前重新核对当前文件与重叠 change，记录最终 baseline；确认已有 capability authority、frontmatter 和他人修改被保留。
- [x] 2.2 按 `prompt-drafts.md` 替换 primary audience 正文，保留尾部工具速记；通过 audience 渲染回归核对责任、依赖、就绪、等待、交接和验收规则。
- [x] 2.3 替换 subagent audience 正文，保留 capability authority 和尾部权限说明；通过 before/after 区段比对及 child 渲染回归确认权限与局部阻塞边界。
- [x] 2.4 替换 general-purpose/explore 的 Markdown role 正文，frontmatter 原样保留；从正常 Agent discovery 路径验证角色差异和共享 audience，覆盖只读与探索停止条件。

## 3. Tool guidance and developer documentation

- [x] 3.1 在 `build_task_description` 增加最小委派信息与后台选择说明，替换不准确的 compacted project instructions 说法；通过动态名字/参数、Agent 覆盖和默认后台测试确认 schema 与执行语义未变。
- [x] 3.2 在 `build_task_output_description` 加入依赖驱动的等待指导；通过重命名参数和 wait-all 描述回归确认没有承诺 wait-any 或新增通知能力。
- [x] 3.3 更新 `PROMPT_ARCHITECTURE.md` 的主从协作和各层所有权说明，必要时更新 `docs/architecture/local-coordination.md` 并链接规范；复核不存在第二份完整 prompt 或进行中任务表。

## 4. Verification and acceptance

- [x] 4.1 更新对应 audience 职责断言并完成实际 Markdown role 的装配覆盖；执行 `validation-plan.md` 中 agent 与 tool-types 有界测试，保存命令、退出码和失败处理。
- [x] 4.2 核对 Full/Extend、角色切换、无 task 工具、child task 简洁约束和保留的权限边界；记录完整渲染后的 prompt 体量差值，不能仅依靠关键词出现判定行为。
- [x] 4.3 复核 E01–E15 的可行性与成本，执行 E01/E02/E11 的真实模型 pilot；保存输入、工具轨迹、实际结果及其他场景未执行的限制，不将未制造的交错记为通过。
- [x] 4.4 固定模型/effort/权限/工具/初始状态做三组 baseline/candidate 配对，记录正确性、耗时、token 和重复劳动；依据契约范围与成本决定停止大规模重复采样，不虚报效率收益。

## 5. Archive after implementation

- [x] 5.1 将所有 delta 场景与确定性验证记录逐项对应，核对 pilot 的覆盖与未执行限制，确认指引契约和文档完成后执行 `openspec validate --all --strict --no-interactive`。
- [x] 5.2 运行 `openspec archive improve-dependency-driven-agent-prompts --yes` 合入规范，再执行全量 strict 与 archived 校验；保存结果，确认未提前合入未实施契约。
