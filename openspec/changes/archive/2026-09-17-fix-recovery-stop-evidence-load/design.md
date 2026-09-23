## Context

sampler 的 `apply_retry_decision` 经 `audit::record` 写入空 body 的 retry 或 recovery_stop。shell 的 `sampling_evidence::sink` 将其持久化为 Timeline Observation；恢复时 `verify_timeline_prompt_blobs_from_directory` 经 `verify` 调用 `decode_record`，导入导出经 `referenced_hashes` 调用同一解码器。当前允许列表漏掉 recovery_stop。

## Goals / Non-Goals

目标是让既有合法证据通过原有读取边界；不改变 provider 行为、不增加数据迁移、不宽松跳过证据校验。

## Decisions

在既有允许列表增加 recovery_stop，继续共用大小、分块、哈希及名称校验。修改集中在解码器，避免分别绕过加载或导出校验。用真实存储加载测试覆盖观察者和写者，用既有二进制证据往返测试覆盖混合 recovery_stop 和 response；负向测试保持未知类型及损坏引用拒绝。

## Risks / Trade-offs

新增类型仍由生产者和读取者分别声明；本次保持最小闭环，不为四个字符串新增跨 crate 类型体系。回归覆盖当前生产者的停止恢复记录。原始会话只读验证，不将私人正文纳入仓库夹具。
