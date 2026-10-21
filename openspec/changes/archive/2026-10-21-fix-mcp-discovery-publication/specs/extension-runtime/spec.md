## ADDED Requirements
### Requirement: MCP discovery consumes complete paginated catalogs
Doctor、会话工具注册和 descriptor 物化 SHALL 复用完整 tools/list 分页收集，保留所有页的工具；分页失败或 cursor 循环 SHALL 明确失败而非报告部分成功。搜索快照 SHALL 继续来自实际已注册工具而非未过滤的远端目录。
#### Scenario: Tool list spans several pages
- **WHEN** 服务返回 32、32、32、16 个工具并逐页给出 nextCursor
- **THEN** doctor 报告 112 个，注册与 descriptor 使用完整目录。
#### Scenario: Empty page continues
- **WHEN** 空页面仍有新的 cursor
- **THEN** 继续读取，只有 nextCursor 缺失才完成。
#### Scenario: Discovery fails after a successful page
- **WHEN** 后续页错误或重复此前 cursor
- **THEN** 返回发现失败，不发布前面页面为完整目录。
#### Scenario: Shared discovery fails while a catalog is already registered
- **WHEN** 仍有资格的共享客户端分页发现失败
- **THEN** 保留上一份完整目录，不能把错误当成成功空目录；服务退休或资格撤销仍清理注册。
### Requirement: MCP initialization publishes to the current Agent bridge
后台 MCP 初始化 SHALL 在远程发现后解析当前 Agent bridge，将注册和搜索快照发布与 bridge 替换协调；网络等待 SHALL NOT 占用该本地发布边界。发布继续遵守当前配置 generation、禁用、model visibility 和继承资格。
#### Scenario: Agent changes during initial discovery
- **WHEN** tools/list 尚未完成时 Agent bridge 被替换
- **THEN** 发现完成后工具注册到当前 bridge，搜索与调用一致，退休 bridge 不接纳迟到目录。
#### Scenario: Agent changes after registration
- **WHEN** 后台完成注册与快照发布后 Agent 被替换
- **THEN** 新 bridge 从完整已观察快照恢复，既有工具仍可查找和调用。
#### Scenario: Obsolete discovery or ineligible tool
- **WHEN** catalog generation 已变更或工具不满足当前资格
- **THEN** 旧发现不恢复被删除服务，不使不合格工具成为可调用或搜索结果。
### Requirement: Missing MCP tool diagnostics describe local registration
MCP 工具查找失败 SHALL 明确指出当前会话未注册该工具；仅在有证据时说明 catalog 可见，不推测服务连接、远端存在性或人工授权。
#### Scenario: MCP name cannot be resolved
- **WHEN** use_tool 的 MCP qualified name 不在当前 dispatch registry
- **THEN** 错误说明本会话注册缺失并引导刷新 search_tool；不重放其他工具或绕过资格。
