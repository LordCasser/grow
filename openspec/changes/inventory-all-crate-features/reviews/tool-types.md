# tool-types 逐包核查

包路径：`crates/common/tool-types`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p tool-types --all-features：102 项通过，覆盖 prompt-render 条件路径；无 doctest。

## 模块与开关

- `crates/common/tool-types/Cargo.toml`
- `crates/common/tool-types/src/behavior.rs`
- `crates/common/tool-types/src/ext.rs`
- `crates/common/tool-types/src/lib.rs`
- `crates/common/tool-types/src/schema_utils.rs`
- `crates/common/tool-types/src/serde_lenient.rs`
- `crates/common/tool-types/src/task.rs`
- `crates/common/tool-types/src/types.rs`

Cargo feature：`{"prompt-render": ["dep:minijinja"]}`。

## 功能与规范映射

- [Tool descriptions preserve raw schema](../specs/tool-runtime/spec.md#requirement-tool-descriptions-preserve-raw-schema)：ToolDescription SHALL 保存 name、description、可选 namespace/title/kind、原始 arguments_schema 和不序列化的 extra；to_input_schema 原样返回原始 schema，缺失时返回空 object schema。
- [Runtime description extensions](../specs/tool-runtime/spec.md#requirement-runtime-description-extensions)：Extensions SHALL 按 TypeId 保存可克隆的 Box 值，支持 set/get/get_mut/remove/contains；克隆时调用值自身 Clone，Debug 只显示长度。
- [Argument type and constraint metadata](../specs/tool-runtime/spec.md#requirement-argument-type-and-constraint-metadata)：ToolArgument SHALL 保存类型、required、default、allowed_values、可选原始 schema 和四种数值边界；新建时类型 String、required=true，设置 default 不改变 required。
- [Lossy schema property projection](../specs/tool-runtime/spec.md#requirement-lossy-schema-property-projection)：parse_arguments_from_schema_lossy SHALL 只遍历顶层 properties，提取 description、required、default、allowed_values 优先于 enum、type 和四种数值边界；不存在对象 properties 返回空。
- [Lenient argument conversion helpers](../specs/tool-runtime/spec.md#requirement-lenient-argument-conversion-helpers)：布尔转换辅助函数 SHALL 接受 bool、整数 0/1、trim 后不区分大小写的 true/false/yes/no/1/0，null 转 false；不识别形式返回 None 或 serde 错误。
- [Behavior wire and display identities](../specs/behavior-goal/spec.md#requirement-behavior-wire-and-display-identities)：BehaviorId SHALL 包含 Normal/Clarify/Plan/Workflow/Goal，默认 Normal；as_id/Display 与 try_from_id 使用 normal/ask/plan/workflow/goal，serde 使用 snake_case 枚举名。
- [Subagent capability meet](../specs/tool-authorization/spec.md#requirement-subagent-capability-meet)：SubagentCapabilityMode SHALL 序列化为 read-only/read-write/execute/all，默认 ReadWrite；intersection 按能力交集收窄，All 为单位，ReadOnly 为下界。
- [Task spawn input and optional sentinels](../specs/tool-runtime/spec.md#requirement-task-spawn-input-and-optional-sentinels)：TaskToolInput SHALL 要求 prompt 与 description，缺省 subagent_type=general-purpose、run_in_background=true，后者使用宽松布尔解析；保留 capability_mode/isolation/resume_from/cwd/model/task_id 可选字段。
- [Task output query normalization](../specs/tool-runtime/spec.md#requirement-task-output-query-normalization)：TaskOutputToolInput SHALL 严格接收 task_ids 字符串数组与可选 u64 timeout_ms，拒绝未知字段；resolved_task_ids trim、去空并按首次出现顺序去重。
- [Task result state and progress signature](../specs/tool-runtime/spec.md#requirement-task-result-state-and-progress-signature)：TaskOutputResult SHALL 保留状态、时间、输出、文件、截断提示及 raw_output_bytes；is_terminal 只将 completed/failed/cancelled 视为终态。
- [Subagent model result formatting](../specs/tool-runtime/spec.md#requirement-subagent-model-result-formatting)：SubagentCompletedOutput SHALL 保留 output、子 Agent ID/type、调用/turn/耗时统计及可选 worktree_path；to_model_text 输出回答、subagent_meta 和含 resume_from 提示的 subagent_result footer。
- [Builtin subagent prompt catalog](../specs/tool-runtime/spec.md#requirement-builtin-subagent-prompt-catalog)：BUILTIN_SUBAGENTS SHALL 按 general-purpose、explore 顺序提供描述、工具片段和提示模板；按名称精确匹配，未知或用户自定义类型返回 None。
- [Task lifecycle description builders](../specs/tool-runtime/spec.md#requirement-task-lifecycle-description-builders)：任务描述 builders SHALL 按调用者提供的工具名、参数名和可用工具生成 task/get_task_output/kill_task 文本，保留传入模板占位符供下游解析。

## 边界

- 收集 name 与 namespace 的所有标识符错误，要求非空且仅 ASCII 字母数字/下划线/连字符；构造和 serde 不自动调用 validate，不校验 schema 内容。
- Display 使用 namespace.name 与非空 description，不使用 title，也不是规范 ID；参数视图有损，未附 schema 则为空。
- Extensions 恒等比较 true，ToolDescription 的相等性忽略这些元数据，serde 跳过 extra；不能据此判断实际 runtime metadata 相同。
- 支持 string/integer/number/boolean/array/object/null；primary_type 取首个非 null，缺失则 Null；nullable 按是否包含 Null，primitive 要求所有成员 primitive，composite/numeric 只需任一成员满足。
- 过滤非法成员，无有效成员回退 String，单个成员归一为 Single；直接 serde 对未知类型返回错误，不等同于 from_value。
- required=false 写入 JSON，true 省略；边界仅为元数据，本层不验证参数值满足限制。
- 不进行完整 JSON Schema 校验；array/object 保留属性原文，普通属性的 pattern/format/组合与条件限制不会成为独立 ToolArgument 约束。
- 提取非空 enum 或 oneOf const，类型由首个值推导，首个枚举值成为 resolved default 并优先于属性 default；anyOf 中的 null 分支可能不保留，结果不能代替原 schema。
- 组合可识别类型并保留属性原始 schema；不解析远程 ref 或任意递归引用。
- 配合 serde default，缺失为 None，显式 null 为 Some(false)；1.0、其他数字、空字符串和对象拒绝。
- 字符串或数字转单元素列表，数组成员只接受字符串/数字，null 为空；bool、对象、嵌套数组拒绝。这些辅助函数不会自动应用到所有 task 输入。
- serde 值为 clarify，外部 ID 为 ask；try_from_id 不接受 clarify 或未知值。
- 只有 Plan/Goal 分类为特殊 runtime；choice 返回首个匹配 entry，entry 含 supported、available/confirmation_required/unavailable 和可选 reason；本包只定义投影，不执行状态转换或授权。
- 返回 ReadOnly，既不扩大也不在本层报错；isolation enum 为 none/worktree，默认 None。
- task_id 被 schemars 隐藏但 serde 可接收；目录存在性、resume 归属和状态、model 选择及 worktree 互斥由启动实现核验，本结构不执行。
- trim 并删除空白、null/none/undefined 的不区分大小写哨兵；serde 不自动调用此函数。
- serde 拒绝；MAX_MULTI_WAIT_IDS=20 为共享常量，本结构本身不限制数组长度。
- 前两者 waits=false，正数 true；raw JSON 辅助函数只认可非负整数，不把数字字符串当等待。
- 每次从 GROW_MAX_WAIT_BLOCK_MS 解析 u64，失败回退 600000，合法 0 也接受；format_wait_cap_ms 秒/分钟向下取整，description 保留 max_wait_ms 占位符由下游解析，本包不执行阻塞或 cap。
- Result 委托内部状态，TaskNotFound 为 false，MultiResult 总为 true，不逐项检查结果。
- 仅哈希 status、exit_code、ended 是否存在和 raw_output_bytes；不使用显示文本长度、duration 或结束时间具体值，不承诺跨版本稳定哈希。
- 仅 Result.outcome==killed 返回 true，already_exited 和 TaskNotFound 为 false；本包不杀进程。
- 后台提示含 ID/type/description 和传入的查询工具名；完成格式化不自动把 worktree_path 加入模型文本，字符串字段直接插值，本层不校验或转义。
- 替换 execute/read/edit/list/search/plan 对应名称，未知占位符回退末段 kind，未闭合占位符原样保留；to_descriptor 保留名称描述并设置工具片段。
- 使用 MiniJinja 自定义分隔符与 tools.by_kind，上下文缺失 kind 留空且条件段可隐藏，失败返回 None；general-purpose 提示限定任务范围和工作区，explore 提示只读，这些文字不替代运行时权限。
- 按列表顺序输出名称/描述/可选工具片段，并说明后台、resume 和 isolation；文案要求指定类型，但输入 serde 仍存在 general-purpose 默认。
- 按配置输出 Job Object 或 SIGTERM/SIGKILL、Cancel+Shutdown 文案，不在本包执行这些动作。
- 合并同名来源表述，缺 read 时不添加读文件提示，等待上限保持占位符，monitor 提示使用单数 ID 参数名。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 验证范围

7 个 Rust 文件和 Cargo.toml 已完整读取。核查包含结构与 serde 差异、宽松解析辅助函数、schema 有损投影、子 Agent 文案与 feature 条件分支。没有从提示文案推导出进程/权限/会话实现保证；这些执行路径仍须在相应 crate 遍历。

关键边界由现有测试锁定：task_output_input_requires_canonical_plural_array、parse_schema_any_of_ref_resolves_enum、description_to_input_schema_prefers_raw_with_defs、render_prompt_resolves_tools_and_conditionals。Behavior 的双身份表示另由枚举 serde 属性与 as_id/try_from_id 的直接实现核对，不能用其他测试通过代替其证据。
