## ADDED Requirements

### Requirement: Forked response history belongs to the new lineage

普通 fork SHALL 在新 Timeline lineage 中保留已继承的响应展示。父 response projection SHALL 经父 Timeline 核对后转换成继承历史，不能把父 admission identity 作为子 Timeline authority；恢复 SHALL 不重新采样或执行工具。

#### Scenario: Fork inherits projected text and reasoning

- **WHEN** 普通 fork 继承父会话的 admitted text、reasoning 及其后工具历史
- **THEN** fork 的 typed/raw/direct replay 各显示一份有序历史，且不携带父 admission/candidate 身份。

#### Scenario: Fork excludes discarded response history

- **WHEN** 父会话含 rewound、quarantined response 或 fork 指定截断点
- **THEN** 子会话只继承所选历史，不复活被排除的 raw candidate；fork_filter 继续清除展示缓存。

### Requirement: Resident replay preserves the physical snapshot frontier

Resident reconnect SHALL 不越过尚未由其 Timeline snapshot 覆盖的 response projection 后再丢弃该记录。初次回放和后续增量 SHALL 共享准确的物理截点，保持 response 和后继更新的顺序及恰好一次交付。

#### Scenario: Response completes between authority and cache snapshots

- **WHEN** resident actor 在 load 取得 Timeline snapshot 之后、读取 updates snapshot 之前提交新 response projection
- **THEN** 初次回放在该物理记录之前截止，delta 在 load 完成前交付该响应和后继工具一次。

#### Scenario: Already synthesized response reaches the physical cache

- **WHEN** Timeline snapshot 已包含 response，初次回放合成它，而物理 projection 稍后追加
- **THEN** delta 不重复展示该 response，后继独立更新仍交付。
