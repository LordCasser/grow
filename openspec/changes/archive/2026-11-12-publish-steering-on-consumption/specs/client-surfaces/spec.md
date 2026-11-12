## MODIFIED Requirements

### Requirement: Interjection display preserves user text across resume
运行中用户输入 SHALL 将用户可见文本与模型专用插话指令分开持久化。显式技能正文 SHALL 来自关联的独立加载结果，不再追加到用户输入或其插话信封。恢复 SHALL 展示保存的用户文本；用户自己写入的标记 SHALL 保留，显示文本 SHALL NOT 通过解析模型提示标签恢复。

The live transcript SHALL render a durable steer only after its input consumption is acknowledged, using the same message identity as history replay. Enqueue or admission MAY show transient feedback. When the target closes before consumption, an admitted steer SHALL retain its input identity through the existing FIFO fallback and appear in the transcript at its eventual delivery boundary. These rules SHALL apply to both a shared queue promotion and a local queued item sent through direct steering. On cancellation, Pager SHALL consider only the cancelled foreground prompt for a pristine rewind. A stashed submission for another input SHALL remain under its existing admission and delivery owner.

#### Scenario: Foreground is cancelled before pending steering is consumed
- **WHEN** the user promotes queued input into a running Turn while a tool blocks its steering boundary, then cancels the Turn before the input is consumed
- **THEN** the live transcript shows no committed steer for the cancelled target and shows the input once when its original identity is consumed after rerouting. Pager does not restore the admitted steer to the composer as a draft.

#### Scenario: Steering is consumed in the original Turn
- **WHEN** a pending steer reaches its safe boundary and Shell durably consumes it
- **THEN** all attached panes can render one committed message using the persisted display identity, and delivery of its matching history echo cannot add another row.

#### Scenario: Resume after accepted steering
- **WHEN** 运行中输入在加载前提满足后被消费，显示更新随后被回放
- **THEN** 显示清理后的原始输入，模型上下文保留适用的插话指令与独立技能结果，不把 Runtime 信封显示为用户文字。

#### Scenario: Direct interjection injection
- **WHEN** 没有队列输入身份的直接插话被接纳
- **THEN** 同样保留显示与模型内容的分离以及图片块；含技能引用时，先建立 Runtime 持有的稳定关联身份，再执行相同加载前提。

#### Scenario: User authors prompt-like markup
- **WHEN** 用户输入包含字面的 user_query 标签
- **THEN** 标签作为用户文字保留，不被当作 Runtime 包装解析或删除。

证据入口：`crates/codegen/shell/src/session/actor/interjection.rs` 的直接注入与 consume_steer_inputs、`crates/codegen/pager/src/acp/tracker.rs` 的持久显示文本回放。
