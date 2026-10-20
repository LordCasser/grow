# 配置入口与 Settings 覆盖审查

## 范围与结论

审查 Settings catalog 的全部 42 个注册 key，逐条核对 current value、Bool/Enum/Int/DynamicEnum commit、reset、persist 和 rollback。另比较 UiConfig 用户偏好、settings_writes helpers、SubagentsConfig 及相邻专用入口。本范围内除已经修复的子 Agent 权限菜单和 show_model_provider 测试清单外，未确认另一项注册链路漏接；不据此声称所有 Grow 配置或平台路径均无缺陷。

Catalog key 与 ALL_SETTINGS_EXERCISED 现在精确对应 42/42，max_thoughts_width 通过其常量解析。键列表：

`compact_mode`, `screen_mode`, `show_timestamps`, `show_timeline`, `page_flip_on_send`, `combine_queued_prompts`, `simple_mode`, `vim_mode`, `theme`, `auto_dark_theme`, `auto_light_theme`, `render_mermaid`, `permission_mode`, `subagents.permission_mode`, `remember_tool_approvals`, `default_model`, `show_model_provider`, `max_thoughts_width`, `show_thinking_blocks`, `prompt_suggestions`, `respect_manual_folds`, `group_tool_verbs`, `display_refresh_auto_cadence`, `scroll_speed`, `scroll_mode`, `scroll_lines`, `invert_scroll`, `keep_text_selection`, `default_selected_permission`, `toolset.ask_user_question.timeout_enabled`, `show_tips`, `contextual_hints`, `auto_update`, `hunk_tracker_mode`, `contextual_hints.undo`, `contextual_hints.plan_mode`, `contextual_hints.image_input`, `contextual_hints.send_now`, `contextual_hints.small_screen`, `contextual_hints.word_select`, `contextual_hints.ssh_wrap`, `fork_secondary_model`

## 已确认并修复

| 问题 | 证据及处理 |
| --- | --- |
| 子 Agent 独立权限没有 Settings 入口 | 后端已有 SubagentsConfig.permission_mode，catalog 原先仅 primary permission。已由 expose-subagent-permission-setting 独立归档补齐。 |
| show_model_provider 的四处 integration 清单遗漏 | 原矩阵、Bool kind、default expected 和 typed setter whitelist 均未登记；补齐四处并增加键盘 Space 与鼠标交互。默认测试和新交互显式复位线程缓存。 |
| 完整菜单 integration 未进入发布回归 | `.github/workflows/core-regression.yml` 原来只执行 library 和其他精确进程/PTY 用例，未运行 settings_e2e。现在添加整组 Settings integration，沿已有同 SHA Core 门禁阻止失败发布。 |

## 不应误报为通用 Settings 漏项

| 配置或边界 | 现有入口与证据 |
| --- | --- |
| cursor_blink | client-support/src/ui_config.rs 字段注释明确为 config-file-only，没有要求菜单入口。 |
| follow_up_behavior | 同文件明确 no pager settings row yet / deferred；队列运行时和配置解析已存在。记为明示延期的菜单能力，不当成测试清单漏项。 |
| mouse_reporting_toggle | 功能开关由配置启用；运行期通过 slash/commands/toggle_mouse_reporting.rs 的显式命令操作。不能把运行期切换当作开关配置持久化。 |
| cancel_subagents_on_turn_cancel | root/dispatch/turn.rs 的 AlwaysStop / AlwaysContinue 提示项写入已有偏好，并有 effects/helpers.rs 与 rollback 路径；不属于 catalog 注册漏接。选定持久偏好后在 Settings 查看/重置的可发现性可另行评估。 |
| subagents.enabled / max_depth / classifier_input / models / toggle | shell/src/config/mod.rs 定义独立能力开关、深度、分类上下文、按名称模型 map 和启停 map；enabled 有 CLI/env/config，toggle 有 Agents modal，其余有配置解析与优先级专项用例。没有足够证据认定都应直接增加通用 Settings 行。 |
| contextual_hints Group | 父节点只展开七个 Bool 子项，无标量 current/reset/persist 是预期；每个 child 有完整路径。 |
| screen_mode rollback 为 String 而注册为 Enum | String 保存 raw Option 的 unset 语义，失败后可还原未配置；有定向队列、失败和重试测试，不能简化成 canonical Enum 而丢掉基线。 |

## 剩余测试覆盖限制

`every_registered_setting_is_exercised` 仅比较 key 字符串集合，不验证测试函数是否实际执行过对应键鼠动作。Bool 有通用 Space typed setter 检查；部分 Enum 鼠标只验证打开 picker，Int/DynamicEnum 与其他 Enum 缺少同等全表交互覆盖。因此 42/42 只证明登记完整，不能宣称每一项的所有交互均已验证。该覆盖债务单独登记，不在本次引入测试生成框架。

## 证据入口

- `crates/codegen/pager/src/settings/{defs,registry}.rs`
- `crates/codegen/pager/src/views/settings_modal/state.rs`
- `crates/codegen/pager/src/app/root/dispatch/settings/{ui,setters}.rs`
- `crates/codegen/pager/src/app/root/effects/helpers.rs`
- `crates/codegen/pager/tests/settings_e2e.rs`
- `crates/codegen/client-support/src/ui_config.rs`
- `crates/codegen/shell/src/config/{mod,tests}.rs`
- `crates/codegen/shell/src/util/config/settings_writes.rs`

结构入口通过 Atlas scoped search 交叉核对 current_value_for（settings scope complete，非全仓调用图完备声明）。运行结果另见 verification.md。
