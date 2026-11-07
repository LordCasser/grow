# Tasks: qualify-stream-error-envelopes

验证方法（实现前约定）：Tier 3。单元测试覆盖混合帧（type/choices/object 三形状、typed 残缺、`event:error` 名覆盖、显式 null 成员）、重复键 fail-closed、bare/named/typed 保持原分类、未知 extension+error 归 Serialization；L2 用真实 axum SSE 服务器经三 reader 的 `conversation_collect` 覆盖普通+error、已有初始工具候选（chat/responses）或已接受普通帧（messages）后混合帧整次拒收。编译与真实red/green由父协调共享Cargo窗口，实际结果见verification.md；`rustfmt --edition 2024 --check`限定自文件，归档前严格校验。

- [x] 1. 建 proposal、delta（WHEN/THEN）、design、tasks；`openspec validate qualify-stream-error-envelopes --strict --no-interactive` 通过。
- [x] 2. error.rs 单元测试：普通身份+error（type/choices/object 三形状、typed 残缺普通部分、event:error 名、error:null/choices:null）、普通无 error 不变、重复 type/error 键（含首 null 后 valid）fail-closed Serialization、bare legacy / 完整 named / 合法 typed 保持原分类、未知 extension+error 归 Serialization。最终父整合窗已运行共享分类42项与sampler client75项，均通过；三reader旧分类反例实际3红，详见verification.md。
- [x] 3. 实现 `try_parse_stream_error` serde 窄 probe 资格门（`MemberPresence` presence wrapper、`IgnoredAny` 忽略内容、probe 解码失败 fail-closed 返回 Serialization 不落回 legacy）。最终父整合窗已运行共享分类42项与sampler client75项，均通过；三reader旧分类反例实际3红，详见verification.md。
- [x] 4. L2：client.rs 三 reader（chat/responses/messages）真实 SSE → `conversation_collect` 回归已写入（chat/responses 含已有初始工具候选后混合帧整次拒收；messages 先接受正常 `event:message_start` 普通帧、仅混合帧用 `event:error`；合法 bare/named/typed SSE 对照保持原分类；`responses_stream_client` 唯一化为 `sse_stream_client` 的委托）。最终父整合窗已运行共享分类42项与sampler client75项，均通过；三reader旧分类反例实际3红，详见verification.md。
- [x] 5. rustfmt（两文件 --check）、`openspec validate --all --strict --no-interactive`、最终 diff 自查（仅本项 hunks，排除 sibling 已归档 fix-usage-normalize 的 client.rs 继承改动与 archive.log 删除）、`/tmp/qualify-stream-error-envelopes.patch`（显式 pathspec，已在 HEAD 副本正向 apply 验证）。不 commit、不归档。
