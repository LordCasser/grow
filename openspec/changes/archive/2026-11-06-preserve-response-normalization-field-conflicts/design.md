# Design: Responses usage completion preserves field conflicts

## 现状与缺陷机制

`decode_response_event`（client.rs）在 typed decode 失败后调用 `normalize_response_event`，优先运行 `normalize_response_usage_details`。现实现把整事件读成 `serde_json::Value`，对 `response.completed|incomplete|failed` 且 `response.usage` 为对象时：

- 明细容器缺失 → 插入 `{cached_tokens|reasoning_tokens: 0}`；
- 容器为对象但计数字段缺失 → 插入 0；
- 容器非对象 → 返回 None（不修复）；
- 有插入 → `value.to_string()` 作为 decode 副本。

`Value` 的 Map 对重复 key 只保留末值，重编码即擦除。于是「缺 usage 明细 + 任意位置重复已知字段」的输入在补全后丢失重复字节，第二次 typed decode 通过，SDK 原本的 duplicate-field / null-type 拒收被绕过；真实 client→L2 会把该 terminal 当作权威完成候选（证据见 proposal）。

## 方案：borrowed `RawValue` 局部补丁

保持「适用性判定」与「补全产物」分离：

1. **判定**：用 `serde_json` 的 `raw_value` feature，把事件按「本 helper 会触碰的层级」做窄借用镜像：顶层 `{type: &str, response: {usage: &RawValue}}`，usage 层 `{input_tokens_details: Option<&RawValue>, output_tokens_details: Option<&RawValue>}`，每个明细容器只镜像自己那一个计数字段（input→`cached_tokens`、output→`reasoning_tokens`）。派生 struct 解码天然拒绝该路径上的重复已知成员（`type`/`response`/`usage`/明细容器名/对应计数字段），与 SDK 的 duplicate-field 拒收同源；每层未列出的成员保持原始字节，不在本层制造过宽的唯一性要求。
2. **定位**：`RawValue::get()` 的文本借用自原始输入。用 checked 指针差 + 逐字节回验（`borrowed_slice_offset`）把 usage 文本（及需要补计数的明细容器文本）换算成原始 `data` 内的字节区间；任何不匹配降级为 `None`（不补全、保持原拒收），全程 safe 代码。
3. **拼接**：decode 副本 = `data[..usage.start] + 补丁后的 usage 文本 + data[usage.end..]`。缺失明细容器作为 usage 的首个成员前插；容器内缺失计数字段在明细原文前插（多个内层补丁按 span 降序替换，先算后改）。除插入片段外逐字节等于原始输入。

### 语义等价与拒收保持

- 副本保留全部原始字节：任何位置的重复已知字段（`response.id`、function `name`/`call_id`、`status`、`output`、顶层 `type`/`sequence_number`、usage 已知字段、null-first `id`）在第二次 typed decode 中仍被 SDK 拒收，即本修复的核心不变量；同形状输入在明细完整与缺失两种情况下拒收一致。
- 判定镜像只列被触碰层级的字段：usage 层重复 `input_tokens`/`total_tokens` 等 SDK 已知键不进镜像，由保留字节交给 SDK 拒收；明细容器内重复的未知扩展键（如 input 容器里的 `reasoning_tokens`）不被镜像误拒，维持 SDK 对 unknown 成员的既有容许。`type`/`response`/`usage`/容器名/计数字段的重复直接在镜像层失败，返回 `None` 并保留原 SDK 错误。
- 补全资格限定对象形状：`usage` 文本与每个已存在的明细容器文本必须以 `{` 开始（`RawValue` 无外围空白且已良构，前缀判定精确），否则返回 `None`。serde 派生 struct 对序列形式的容许（`[0]` 可解码为 `u32` 成员）因此不会把非对象形状带进补全：SDK 直接解码对完整序列形式的既有容许保持不变，序列形式与另一明细缺失并存时保持该缺失的原有拒收（与旧 Value helper 的 object-only 行为一致）。`null` 容器仍由 `MemberValue::Null` 直接返回 `None`；`type` 缺失/非字符串同理。

### `raw_value` feature 事实与写范围

离线核查 Cargo.lock 图：`agent-client-protocol`/`agent-client-protocol-schema` 无条件启用 serde_json `raw_value` 但不在 sampler 依赖图内；`axum` 仅在其可选 `serde_json` 依赖（`json` feature）下启用且属 dev-dependencies，`cargo build -p sampler` 的 lib 图无人启用。父据此把写范围扩至 `crates/codegen/sampler/Cargo.toml`，在既有 `serde_json` 依赖上显式加 `features = ["raw_value"]`（仅 feature，无新依赖/版本，lock 不变）。borrow `&'a RawValue` 的 `from_str` 借用语义由 serde_json 官方支持；偏移定位仍以逐字节回验兜底。手写 span 扫描器方案已按父决策否决。

### 不做的事

- 不为 Responses schema 复制递归字段清单（不重演 arguments/annotations 的字段镜像法，terminal Response 嵌套太深）。
- 不全局禁止重复 JSON key、不放宽 terminal 校验、不补 summary/status/timestamps、不新增 capability 或共享实体/依赖。
- 不改 `sampling-types/error.rs`、stream 错误预解析（另任务）、不动 arguments/annotations/sparse-failed 三个既有例外。

## 验证设计（Tier 2）

- 原始字符串 fixture（禁止用 `Value` 预构造重复 key）成对覆盖：完整明细 vs 缺明细 × {无冲突、重复 `response.id`、重复 function `name`、重复 usage 已知字段、null-first `id`}。断言：明细完整与缺失两种输入的 decode 拒收一致；缺明细 + 冲突不再产出 `ResponseCompleted`；正常化输出保留重复字节（证明逐字节保留）。
- 真实链路 red/green：loopback HTTP/SSE（既有 `responses_stream_client`）→ `conversation_collect`：
  - 正控缺明细（无冲突）→ 完成、`test_tool` 候选、usage 100/5/105、cache read 保持 None（availability 区分）；
  - 重复 name/id + 缺明细 → 必须 `SamplingError::Serialization` 拒收、无工具候选（当前代码为接受，即 red）；
  - 初始 arguments 例外候选 + 冲突 terminal → 仍拒收；
  - 完整明细 + 冲突 → 拒收（对照不变）。
- 明细 `null`/非对象 → 仍拒收；unknown 扩展重复键 → 维持接纳。
- 既有 client 回归（arguments/annotations/sparse-failed 相关）保留失败断言并串行复跑；`sampling_auth_logs_omit_credentials` 等不删除。
- Cargo 仅在父释放 freeze 窗口后运行：`PATH=/Users/lordcasser/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo +1.93.1 test --locked --offline -p sampler --lib <相关suite> -- --test-threads=1`，长命令后台。
