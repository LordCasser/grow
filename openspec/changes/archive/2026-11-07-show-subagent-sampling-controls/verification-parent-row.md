# 验证记录：父会话 `set_subagent_model` 同一行状态

范围：Pager 主工具行投影（delta requirement `Parent sampling tools show dispatch completion in place`）。Shell 侧控制回执（S 任务）与跨模块终态 notice 不在本记录范围。

## 实现位置

| 位置 | 内容 |
| --- | --- |
| `crates/codegen/pager/src/acp/tracker.rs` | 私有投影 `is_set_subagent_model_tool` / `subagent_model_request` / `subagent_model_tool_call_to_block`，在 `tool_call_to_block` 中紧跟 `communication_tool_call_to_block` 挂接。 |
| `crates/codegen/pager/src/scrollback/blocks/tool/other.rs` | `is_communication()` 增加 `set_subagent_model → ` 前缀，使该行复用既有紧凑通信表头（状态独立换行、不被宽度 gate 丢掉）。无新增公共 block、字段或 layout 配置。 |

## 状态与语义

| 观察 | 条件 |
| --- | --- |
| `Dispatching` | `Pending` / `InProgress`（Shell preparation update）。 |
| `Queued` | `Completed` 且 typed `ToolOutput::SetSubagentModel.status == queued` 且 `subagent_id` 与 `raw_input.subagent_id` 一致。 |
| `Failed` | tool call `Failed`，保留原错误文本/`message`，不从散文推断 `Rejected`。 |
| `Unconfirmed` | `Completed` 但输出缺失、形状不符或目标不一致。 |

`Queued` 只表达目标 actor 接纳；展开文本明确写出「applies at its next closed Step」，不出现 Applied/switched。

身份识别只接受 `raw_input.variant == SetSubagentModel` 或 `_meta["grow/tool"].name == set_subagent_model`；Shell 的 ACP 标题散文（`Queue subagent … model: …`）本身不构成身份。

紧凑摘要把状态放在最前（`Queued · model … · effort …`），窄宽度下状态先于长 model id 出现；effort-only 只显示 effort，不重复子会话最新的 desired model。

## 测试（`tracker.rs` 相邻 tests）

| 测试 | 覆盖 |
| --- | --- |
| `set_subagent_model_queues_in_place_on_the_same_row` | start → `InProgress` → 真实 producer `shell::session::acp_conversion::acp_tool_update` 生成的 `Completed`；同一 `EntryId`、`len() == 1`、summary 状态与请求字段、展开含 Target/Requested/Status/原始参数与结果，且无 Applied 类断言。 |
| `set_subagent_model_completion_without_typed_admission_is_not_queued` | 无输出、非 typed 形状、目标不一致三种情况都不进入 `Queued`。 |
| `set_subagent_model_failure_keeps_the_original_error_in_place` | 失败留在原行、`Failed` 摘要、原错误保真。 |
| `set_subagent_model_title_lookalike_stays_a_generic_row` | 仅标题相同保持 generic；`grow/tool` 身份可投影，但无 typed 输出时不显示 `Queued`。 |
| `repeated_set_subagent_model_completion_keeps_one_row` | 重复 completion 不新增行、同一 `EntryId`。 |
| `set_subagent_model_effort_only_summary_names_only_effort` | effort-only 摘要只含 effort。 |
| `set_subagent_model_status_stays_readable_on_a_narrow_row` | 40/80 列折叠渲染中状态、工具身份与目标均可见。 |

终态 shape 由 Shell 的 public producer 生成，避免 Pager 手写与真实 wire 不一致的 `raw_output`。

## 父实际验证

父在最终稳定源码上执行，均使用Rust `+1.93.1`、`--locked --offline`、`CARGO_BUILD_JOBS=2`、`RUST_MIN_STACK=16777216`，PATH显式将`~/.cargo/bin`置前，并清除临时target-dir/compiler/flag环境覆盖：

- `cargo +1.93.1 test -p pager --lib --locked --offline set_subagent_model -- --nocapture`：actual exit0，**7 passed / 0 failed**，日志`/tmp/grow-subagent-control-parent-final-green.log`。
- `cargo +1.93.1 test -p pager --lib --locked --offline acp::tracker::tests:: -- --nocapture`：actual exit0，**180 passed / 0 failed**，含上面7项，日志`/tmp/grow-subagent-control-parent-final-regress.log`。两组有重叠，不相加为独立测试数。

最初父构建已成功运行七项，6 passed / 1 failed：窄屏下目标按正常word-wrap显示为`child-\nalpha`，测试错误要求跨行连续substring。修正仅将目标断言沿渲染glyph序列拼接核对；Queued、工具身份、同EntryId/单行数、完整参数与错误断言保留。此失败属于fixture，不作为生产旧红。日志`/tmp/grow-subagent-control-parent-first.log`保留。

中间子执行者的Homebrew cargo不支持`+1.93.1`，以及编译环境不一致造成的全链重编均未形成产品green；父终止该owned命令并核对应cargo/rustc PID退役，未清target。诊断日志`/tmp/grow-subagent-control-parent-green.log`保留，不视为产品验证。最终上述两命令才是本次P验收证据。

父已亲核typed producer/consumer与最终7项、180项输出，接受P功能模块；未声称独立产品旧红。单个文件仍有既有dirty区域的格式差异，本轮不做跨owner格式化；仅新feature区域的格式整理另在交付前完成。真实TTY导航、parent排队到child实际Applied的同产物显示仍待产品用例执行，不能以模块通过代替里程碑。
