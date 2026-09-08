# extension-types 逐包核查

包路径：`crates/codegen/extension-types`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；已执行 cargo test --locked -p extension-types，22 项测试通过、0 失败。

## 模块与开关

- `crates/codegen/extension-types/Cargo.toml`
- `crates/codegen/extension-types/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Extension DTO wire naming](../specs/extension-runtime/spec.md#requirement-extension-dto-wire-naming)：extension-types SHALL 提供 hooks/plugins/MCP/marketplace 的 serde DTO，普通 struct 使用 camelCase，动作和 origin 使用 type 标记与 snake_case variant，variant 内字段保持 snake_case；本层不执行动作或验证领域身份。
- [Hook metadata and action vocabulary](../specs/extension-runtime/spec.md#requirement-hook-metadata-and-action-vocabulary)：HookInfo SHALL 表达 name、event、handlerType、matcher、command、url、timeoutMs、sourceDir、disabled，HooksListResponse 同时提供 projectTrusted 和可省略的空 loadErrors；disabled 缺失默认 false。
- [Plugin identity origin and displayed inventory](../specs/extension-runtime/spec.md#requirement-plugin-identity-origin-and-displayed-inventory)：PluginInfo SHALL 拒绝未知顶层字段，要求 origin，并携带 id/name/root/scope/enabled、版本描述、skill/agent 名称和数量、hook/MCP 状态数量以及可选 conflict；scope 为 cli/project/user/config。
- [Plugin action confirmation field](../specs/extension-runtime/spec.md#requirement-plugin-action-confirmation-field)：PluginsAction SHALL 表达 reload、install(source)、uninstall(plugin_id,confirmed)、update(可选 plugin_id)、add/remove(path)、enable/disable(plugin_id)。
- [MCP extension list display state](../specs/extension-runtime/spec.md#requirement-mcp-extension-list-display-state)：McpServersListResponse SHALL 提供 servers，每项含 name/enabled/toolCount，及可缺失的 status/tools/configSource；status 为 ready/initializing/unavailable，tool 包含 name 和可省略 description。
- [Plugin component categories and summaries](../specs/extension-runtime/spec.md#requirement-plugin-component-categories-and-summaries)：PluginComponents SHALL 按 skills、commands、agents、mcpServers、hooks、lspServers 六类顺序枚举，缺失列表默认空且空列表不序列化；summary_line 只包含非空类并按数量使用英文单复数，以中点连接。
- [Explicit catalog component sanitization](../specs/extension-runtime/spec.md#requirement-explicit-catalog-component-sanitization)：ComponentItem::new 与 PluginComponents::sanitize SHALL 去除 is_control 字符及 U+200B..200F、202A..202E、2066..2069、FEFF，将 name/description 各截为最多 120 Unicode scalar 字符，清洗后空 description 变 None；sanitize 每类仅保留前 50 项。
- [Marketplace catalog wire records](../specs/extension-runtime/spec.md#requirement-marketplace-catalog-wire-records)：MarketplaceListResponse SHALL 提供 sources，每项保存 sourceName、featured、sourceKind、sourceUrlOrPath、plugins 和可选 error；plugin entry 表达名称、版本描述/分类/作者、tags/keywords/domains/homepage、relativePath、installStatus/installedVersion、可选 components 与 remoteUrl/ref/sha/subdir。
- [Marketplace action identity](../specs/extension-runtime/spec.md#requirement-marketplace-action-identity)：MarketplaceAction SHALL 表达 refresh(可选 source_url_or_path)、install/update/uninstall(source_url_or_path,plugin_relative_path)、add_source(url)、remove_source(source_url_or_path)，封装于带 sessionId 的请求。

## 边界

- 外层 sessionId 与 action 必需，内层 hook_name/plugin_id/source_url_or_path 保持下划线；字符串格式由 caller 验证。
- status 为 success/validation_error/confirmation_required/not_found/internal_error/unsupported；message、requiresReload、requiresRestart 为独立必需字段，不由 status 自动推导。
- 支持 session_start/session_end/stop/stop_failure/stop_cancelled、pre_tool_use/post_tool_use/post_tool_use_failure/permission_denied、user_prompt_submit/notification、subagent_start/subagent_stop、pre_compact/post_compact；Display 使用人类可读标签，与 wire string 分离。
- 支持 reload/trust/untrust、按 path add/remove、按 hook_name enable/disable、按 hook_names 与 disable 的 toggle_source；不在 DTO 验证 command/http 字段互斥或 matcher 可执行性。
- 支持 cli_override/project_grow/user_grow/config_path/marketplace_install；marketplace_install 的 source_name/git_url 可缺失且 None 时省略；未知 origin variant 拒绝。
- 名称列表空、hookCount=0、conflict=None；空名称列表和 None conflict 省略，数量不根据列表重新计算。hookStatus/mcpStatus 为 active/active_inline/blocked/none，DTO 不自行计算信任。
- confirmed 默认为 false；字段本身只表达 caller 输入，不等于本层获得授权或执行卸载。
- 可解析为 None；实际选取与更新由领域 handler 决定。
- status/configSource 为 None、tools 为空，序列化省略这些空字段；toolCount 不与 tools 长度自动核对，MCP 连接和能力发现不在本包实现。
- is_empty 为 true、summary_line 为 None；Some(empty components) 与没有 catalog inventory 的 None 可以区分。
- 保存 name 与可选 description，hooks 的 name 可表示事件、description 可表示 matcher；DTO 不验证组件在磁盘实际存在或数量一致。
- 不会自动 sanitize；渲染 consumer 必须在入口显式调用，空 name 不被拒绝，不去重也不 trim 普通空格。
- 仅遍历 source.plugins 中 Some(components) 并清洗组件；不清洗 plugin.name/description、source label、URL 等其他字符串，也不限制 source/plugin 数量。
- tags/keywords/domains 为空、homepage/components/remote 字段为 None、featured 默认为 false；这些 DTO 不拒绝未知字段。installStatus/sourceKind 保持普通字符串，不在本层枚举或验证 SHA。
- 仅说明 wire 携带库存，SHA 验证必须由 producer 执行；本 serde 类型不提供真实性证明。
- 解析为 None，表达未限定源；路径规范化、URL 准入、安装和删除的实际语义继续在 handler 与 marketplace crate 核查。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 验证范围

本包仅一个 Rust 文件和 manifest，全部已读。`cargo test --locked -p extension-types` 退出 0，22 项 serde/清洗/目录分类测试通过；日志 `/tmp/grow-extension-types-tests.log`。未运行 shell/pager 的领域 handler，不据此声称 install/trust/reload 或 marketplace SHA 校验已实现。

测试名中的 every_consumer_path 只测试此包 categories/summary/sanitize 三个入口，不能证明所有下游 consumer 已覆盖。HookEvent serde 测试表未列 StopCancelled，但枚举和 Display 存在该变体，规范保留完整 15 类事件，不把测试表当全集。

PluginInfo 的 deny_unknown_fields 不适用于所有 DTO；动作 variant 中的下划线字段也不同于请求外层 camelCase。MarketplaceListResponse.sanitize 仅处理 components，这与全响应字符串清洗不同。下游入口是否调用与其他字符串呈现方式继续在所属 crate 核查。
