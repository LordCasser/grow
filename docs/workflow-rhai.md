# Workflow Rhai 写作参考

> **Status**: 参考文档
> **Date**: 2026-08-13
> **Scope**: shell / workflow 的公共 Workflow Definition 脚本语言
> **对象**: 在 Workflow behavior 中 draft 与编辑 `.grow/workflows/*.rhai` 的主 Agent 及用户

公共 Workflow Definition 是确定性 Rhai 脚本：一个脚本把工作拆成若干子 Agent 调查、
实现或验证任务并编排它们的执行，通过 journal 重放实现暂停/恢复。本页是脚本作者需要的
完整语言契约；Definition/Run 生命周期（draft/validate/run/publish）见
[workflow-workspace.md](architecture/workflow-workspace.md)。

## 1. 骨架与 meta 契约

脚本第一条语句必须是 `let meta = #{ ... };`，字段名固定，多余字段会被拒绝：

```rhai
let meta = #{
    name: "review-changes",
    description: "Review a diff with two independent perspectives",
    when_to_use: "Code review before merging",
    phases: [
        #{ title: "Review", detail: "Two perspectives inspect the diff" },
        #{ title: "Report", detail: "Merge the two reviews" },
    ],
};
```

| 字段 | 约束 |
| --- | --- |
| `name` | 必填；小写 ASCII 字母/数字，单连字符分隔；≤ 64 字节 |
| `description` | 必填非空；≤ 1024 字节；Definition 搜索与参数解析都依赖它 |
| `when_to_use` | 可选；≤ 2048 字节；参与 Definition 搜索匹配 |
| `phases` | 可选；≤ 64 项；`title` ≤ 128 字节，`detail` ≤ 1024 字节 |

`draft` 动作会立即解析 meta；缺失、非法 name 或超长字段会直接拒绝草稿。

## 2. 全局变量 `args`

Run 时通过工具的 `args` 传入任意 JSON，绑定为脚本全局 `args`：

- 对象按 key 访问：`args.query`；不存在的 key 得到 `()`（不是错误），用 `args.query != ()` 判断。
- 未传 args 时 `args == ()`，可借此做必填检查。
- 脚本必须确定性：`eval`、`import`、`timestamp()`、`sleep()`、`exit()` 均不可用；
  需要外部信息请通过 `args` 传入。

## 3. 编排 API

```rhai
phase("Verify");                            // 切换当前阶段（UI 与 tracker 可见）
log("started verification");                // 写运行日志
let r = agent("prompt");                    // 单个子 Agent，阻塞至完成，消耗 1 个 agent_budget
let r = agent("prompt", opts);              // 带选项的单个子 Agent
let rs = parallel([opts1, opts2, ...]);     // 平铺扇出，≤ 1024 项，每项消耗 1 个 agent_budget
complete(#{ report: "...", pass: true });   // 成功结束；value 成为 Run outcome
complete();                                 // 成功结束；outcome 为 null
pause("user", "message");                   // 以暂停结束本次执行
await_user("verification", "message");      // 首次调用暂停；恢复时重放并继续执行
budget();                                   // 返回 #{ total, spent, reserved, remaining }
```

- `parallel` 结果与输入顺序一一对应；条目通常是 AgentResult（见下），失败条目可能是
  `()` 或非 `success`——使用 `output` 前必须检查 `success`。
- `pause`/`await_user` 的 kind 枚举：`user`、`back_off`、`no_progress`、`verification`
  （别名 `blocked`）、`infra`。
- 运行限制：单 Run 默认 agent_budget 128（上限 1024），max_concurrency 默认 3（上限 16），
  每个 session 最多 4 个活跃 Run。

## 4. Agent 选项与结果

AgentOpts（`agent` 的第二参数，或 `parallel` 的条目）：

| 字段 | 含义 |
| --- | --- |
| `prompt` | 任务提示；`agent(prompt)` 的第一参数，`parallel` 条目内必填 |
| `label` | 子 Agent 显示名，如 `"reviewer-0"` |
| `model` | 指定模型；缺省用默认 |
| `max_output_tokens` | 输出 token 上限 |
| `agent_type` | 子 Agent 类型 |
| `capability_mode` | 能力围栏：`"read_only"` / `"read_write"` / `"none"` |
| `isolation_worktree` | 在隔离 worktree 中执行（bool） |
| `fork_context` | fork 主上下文（bool） |
| `resume_from` | 复用既有子 Agent |
| `output_schema` | JSON Schema 对象；`output` 会按它结构化 |
| `phase` | 归属阶段名 |

AgentResult：

```rhai
#{
    agent_id: "...",
    success: true,       // 必须检查；false 时 output 不可信
    output: #{ ... },    // 有 output_schema 时为结构化 JSON；否则为文本字符串
    cancelled: false,
    tokens_used: 1234,
    duration_ms: 8200,
}
```

`output_schema` 是标准 JSON Schema：采样前即强制模型输出该结构，无需再解析自由文本。
对只读调查类子 Agent 显式设置 `capability_mode: "read_only"`；缺省值是 `read_write`。

## 5. Host 工具函数

```rhai
write_scratch_file(name, content)  // → 返回可展示给用户的文件路径
read_scratch_file(name)            // → 文件内容
git_diff_since(commit)             // → 相对 commit 的 diff 文本
render_template(name, vars)        // → 渲染后的字符串
diagnostics_event(name, fields)    // 记录结构化诊断事件
fingerprint(text)                  // → 文本指纹哈希
json_encode(value)                 // → JSON 字符串
```

## 6. Rhai 语言要点

- map 字面量 `#{ key: value, "string-key": value }`；数组 `[]`；`fn name(a) { ... }`。
- 控制流：`if`/`else`、`while`、`for x in arr`、`try { } catch (e) { }`。
- 字符串：`+`、`+=`、`.to_string()`、`.trim()`、`.split("x")`、`.sub_string(i, n)`、
  `.contains(s)`、`.len()`；数组 `.push()`、索引、`.len()`。
- 内置函数：`type_of(v)`、`parse_int(s)`。
- `let` 声明后用 `=` 重新赋值；未声明变量不可赋值。
- 比较 `==`/`!=`/`<`/`>`/`>=`/`<=`，逻辑 `&&`/`||`/`!`。

## 7. 最小完整示例

```rhai
let meta = #{
    name: "dual-review",
    description: "Review a diff from implementation and security perspectives",
    phases: [
        #{ title: "Review" },
        #{ title: "Report" },
    ],
};

let reviewers = [
    #{
        prompt: "Review the diff for correctness and maintainability.",
        label: "impl-reviewer",
        capability_mode: "read_only",
        phase: "Review",
    },
    #{
        prompt: "Review the diff for security vulnerabilities.",
        label: "sec-reviewer",
        capability_mode: "read_only",
        phase: "Review",
    },
];

phase("Review");
let results = parallel(reviewers);

phase("Report");
let notes = [];
for result in results {
    if result != () && result.success {
        notes.push(result.output);
    }
}
complete(#{ reviews: notes });
```

## 8. 常见失败与处理

- `meta` 不是第一条语句、name 非法或字段超长：`draft`/`validate` 立即报错，错误信息带字段名。
- `parallel` 超过 1024 项或 agent_budget 用尽：启动前拒绝或运行中预算暂停。
- 没有 `output_schema` 时 `result.output` 是普通文本字符串。
- Run 是 Definition 内容 hash + 启动 args 的不可变快照：改脚本只影响下一次 Run，
  已在运行的 Run 继续用旧快照。
- 发布前必须用代表性 args 通过 `validate`；发布要求显式 `project` 或 `user` scope。
