# Tasks

- [x] 读取 OpenSpec 索引、model-sampling 现行契约（三个既有 decode 例外 Requirement）、backlog 登记与 `2026-11-06-analyze-sampling-decode-residuals` 证据，确认本项独立边界。
- [x] 建立 change：proposal、delta spec（WHEN/THEN 场景）、design（borrowed `RawValue` 方案与 raw_value feature 事实）、tasks。
- [x] `openspec validate preserve-response-normalization-field-conflicts --strict --no-interactive` 通过（实现完成后复验再次通过）。
- [x] 实现：`crates/codegen/sampler/Cargo.toml` 在既有 `serde_json` 依赖上显式启用 `raw_value` feature（父授权写范围扩展，无新依赖/版本，lock 不变）；`normalize_response_usage_details` 改为 borrowed `RawValue` 窄镜像判定（`MemberValue` 区分 absent/null/Raw）+ checked 偏移回验 + 原始字节局部拼接。
- [x] rustfmt 仅本文件；自检 diff 只触碰 `crates/codegen/sampler/{Cargo.toml,src/client.rs}` 与本 change 目录，`git diff --check` 干净。
- [x] 新增回归（原始字符串 fixture，禁止 Value 预构造重复 key）：
  - [x] 单元：9 类重复已知字段（response.id / function.name / function.call_id / usage.input_tokens / response.status / response.output / 顶层 sequence_number / 顶层 type / null-first response.id）在完整与缺明细两种输入下一致拒收；正常化副本保留重复字节并仍被 SDK 拒收；合法缺明细仍完成且 cache availability 区分保留（raw presence metadata：缺明细 unknown、显式 0 记 "0"）；「仅插入两个明细成员」的逐字节等价断言；明细容器 null/非对象不修；明细内 count null/错类型/负值保留原字节且最终 decode 拒收；unknown 扩展重复键（顶层与 usage 内）维持接纳。
  - [x] 真实链路：`responses_stream_client` + `conversation_collect` 六用例（正控完整/缺明细 + duplicate name/id 各自完整/缺明细对照，冲突用例断言错误含 `duplicate field`）；初始 arguments 例外候选 + 冲突 terminal 整体拒收及其合法缺明细对照完成。
- [x] 父释放的 Cargo 窗口内（Rust 1.93.1、`--locked --offline`、串行）完成 old-red（6/6 失败，缺陷按设计复现）→ green（6/6 通过）→ 全量 `client::tests::` 串行回归（69/69 通过）；实际命令、task id 与结果记录于 verification.md；已向父释放窗口。
- [x] 父 review 返工：usage 与已存在明细容器的 object-only 补全资格门（`{` 前缀判定，RawValue 无外围空白）+ 成对回归 `usage_detail_completion_qualifies_only_object_containers`（完整数组 SDK 正控 + 数组与另一明细缺失并存拒收）；delta 场景与 design「任何非对象都既有拒收」过强说法改为 object-only 补全资格、SDK 原本可直接解码不改写。
- [x] 父在最终 object-gate 源码上重跑 `client::tests::` 串行回归，70 passed / 0 failed；包含新成对数组资格测试，保留全部旧边界及 L2 对照。
- [x] 交付待 review 的 diff、源码 hash、red/green 证据与剩余限制；不归档。
