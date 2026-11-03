# v2.6.0 发布阻塞闭合设计

## 范围

本 change 只协调已确认的 v2.6.0 发布阻塞与验收证据，不实现产品行为、不更改主规格。创建时 backlog 中的八项非延期债务原样保存在 [`backlog-before.md`](backlog-before.md)；每项 owner、最终本机结果和残余边界集中记录在 [`verification.md`](verification.md#八项闭合矩阵)。活动 backlog 现只保留九项明确暂缓事项。

八项由各自独立 change 处理。已归档的行为/测试 changes 仍按其归档路径引用；子任务观察跨平台测试 change 保持 active，不能将本机结果扩写成原生跨平台证明。

## 闭合原则

- 用最终候选的实际命令、测试结果、日志路径和二进制身份判断；中间失败作为历史保留，不能覆盖最终结果或冒充最终失败。
- 区分本机通过与仍待 native CI 的边界。明确暂缓的 ProcessScope owner/reap 身份并发验证继续留在 backlog，不因 terminal 子进程回归通过而隐式关闭。
- 资源分析没有确定来源的全局 RAM cap；高资源实验预算必须来自调用方或宿主。不得从工具 timeout、机器内存或 cgroup headroom 推导产品默认值。
- 普通本地 release build 与 code-signature 验证不能作为 official distribution asset，也不能证明 release workflow、tag、提交 provenance 或平台资产门禁通过。

## 发布门禁

本机已记录 18-crate 核心测试、workspace check、格式检查、Settings E2E、active/archive strict OpenSpec、release-script 单测与普通 release build 的结果，具体数值和日志见 `verification.md`。正式发布仍要求在提交的候选源码上核对 annotated `v2.6.0` tag、执行 `scripts/validate-release.sh v2.6.0`、通过 GitHub required gates/native platform builds 并确认资产 provenance。用户此前已授权全部门禁通过后正式发布；本 change 不要求用户替代执行，也不把未完成的 CI/tag/asset 结果预先标为通过。
