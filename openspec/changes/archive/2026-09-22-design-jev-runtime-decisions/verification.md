# 方案验证记录

日期：2026-09-22。范围：设计文档交付，没有运行时实现。

## 资料与源码核对

- 通过 `read_thread` 读取两段引用讨论，按需求背景使用，不把讨论中的产品宣传与性能估计直接当作事实。
- 检查 TypeSafe 官方 introduction、Choice、confidence 和发布说明；方案中的产品事实带直接链接，Grow 策略明确作为设计建议。
- 阅读 OpenSpec 索引、相关主规范、开发指南和相邻活跃 change；按 `skip_specs: true` 保留纯设计 change，不创建虚构 delta。
- Atlas 已 open 当前项目；scoped search 用于定位。`should_flush` incoming query 的 closure 返回为空且注明范围限制，因此用实际源码调用点确认 `maybe_pre_compaction_flush`，没有据此声称无调用方。
- 一个 Luna xhigh 只读子任务摘录规范与测试入口，主线程核对关键实现及调用方。未运行这些运行时测试，本文不宣称行为验证完成。
- HEAD 为 `3024dad1`，开始时已有大量未提交修改。方案基于当前工作副本；本任务写入范围仅为本 change。工作副本在调查期间仍有其他修改活动，后续实施必须重新核对。

关键源码 SHA-256（设计复核时采集）：

```text
66197d734881a3fc474f026707681b3e9d38bceb54f645127b8129f167865d72  crates/codegen/shell/src/agent/subagent/handle_request.rs
71c4fe3ae73ef2658b4e82390a7cb7cb8a5f0c48ab77a6e704cd21646db463e4  crates/codegen/shell/src/session/actor/turn/sampling.rs
1afbc6cac8c073db0e221607e37c6b265382e9355ba5ba46f672f4ab959a2e46  crates/codegen/shell/src/session/actor/turn/mod.rs
7be43da0aad701f51ea598319f37f58b8bee42aa4b6222d4946348dc53e3e927  crates/codegen/shell/src/session/actor/memory_dream.rs
6fd25ab7fd516abb517cb04f66d562a51714f990b632459cd08bb24996d482cf  crates/codegen/shell/src/session/actor/compaction.rs
828ae07b076f93ae83cddd56f0c305f109facd2ef9db861f3ed8ad3dd5764add  crates/codegen/workspace/src/permission/auto_mode.rs
d2ebab60af9cbb8817b36f0afe558b2a37b84daa740336a5100bb2e756d011c3  crates/codegen/workspace/src/permission/manager.rs
ef9fe0b8d083856c7b6f21c5f8ab44481beade6a061dc857f21f6f6c858df02e  crates/codegen/chat-state/src/sideband.rs
```

现状与提议已区分。fork/resume/classifier route 的部分行为仅由实现完整表达，已在设计记录这一规范覆盖边界；本次未发现并证明需要随方案修复的独立运行时缺陷。

## 文档检查

- `openspec validate design-jev-runtime-decisions --strict --no-interactive`：通过。
- `openspec validate --all --strict --no-interactive`：归档前 22 项通过，0 失败。
- `git diff --check`：通过。新增文件另检查末尾空白和 Markdown 本地链接。
- 设计包括四域输入/输出、执行边界、异常回退、已有代码落点、测量对照与未来 WHEN/THEN 验收草案。

## 未执行

没有调用 Jev 推理、没有发送项目内容给 TypeSafe，没有运行 Cargo 测试或性能实验。阈值、模型档位与收益均待实施阶段基线评测；OpenSpec 格式通过不证明未来策略有效。

## 归档复核

- `openspec archive design-jev-runtime-decisions --skip-specs --yes`：完成，未更新主规范。
- 归档后 `openspec validate --all --strict --no-interactive`：21 项通过，0 失败。
- `openspec validate --archived --no-interactive`：351 项通过，0 失败。
- 对归档后的 6 个本地规范链接、Markdown 末尾空白和 tasks 完成状态进行检查：通过。
- 本 change 的完成只指方案交付与文档校验，不指未来实施清单完成。
