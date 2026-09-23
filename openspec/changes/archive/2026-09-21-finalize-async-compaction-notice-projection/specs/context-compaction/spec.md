## ADDED Requirements

### Requirement: Async compaction completion reports a materialized request projection

异步压缩的用户可见完成通知 SHALL 等下一次普通模型请求完成 request projection 后再发布，并以该投影作为 `tokens_after`。Surface replacement 后、请求重新物化前的中间估算 SHALL NOT 作为异步压缩最终值展示。每次成功提交最多发布一个完成通知；同步提升、同步自动压缩和手动压缩保留各自的即时完成语义。

#### Scenario: Between-step request materializes after async commit

- **WHEN** 异步压缩在闭合 Step 边界成功提交，随后普通后继请求完成组装
- **THEN** 提交与请求组装之间没有异步完成通知；组装后发布的 `tokens_after` 等于该请求已应用的 ChatState projection。

#### Scenario: Completed turn has no next request yet

- **WHEN** 异步压缩在已结束 turn 的边界提交，尚无后继普通请求
- **THEN** 不展示 Surface-only 中间值，完成通知保持待发布，且其间不启动另一轮后台压缩。

#### Scenario: A later turn materializes the next request

- **WHEN** 待发布通知存在，后续 turn 的首个普通请求完成 projection
- **THEN** 使用该 projection 发布恰好一次异步完成通知；后续请求构建或边界检查不重复发布。

#### Scenario: Request construction fails before projection

- **WHEN** 下一普通请求未能完成构建和 request projection
- **THEN** 不发布带猜测 `tokens_after` 的完成通知，也不消费待发布记录。

#### Scenario: A later replacement supersedes the pending display

- **WHEN** 待发布期间同步压缩、手动压缩或 rewind 在下一普通请求前替换上下文
- **THEN** 丢弃旧异步压缩的待发布通知，不把新 replacement 的 token 数归因给旧异步事务。
