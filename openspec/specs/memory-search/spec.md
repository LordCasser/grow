# memory-search Specification

## Purpose
定义 Markdown 记忆的存储隔离与查询结果边界。覆盖默认工作区目录、显式 flat root、embedding 不可用时的文本检索降级，以及合并评分后按配置限制输出的行为。

## Requirements

### Requirement: Scoped memory storage
默认 MemoryStorage SHALL 将全局记忆和工作区记忆置于 memory root，并由工作区标识隔离项目目录。

#### Scenario: 初始化存储
- **WHEN** 通过 MemoryStorage::new 创建工作区记忆
- **THEN** global_dir 使用 override 或 grow_home/memory，workspace_dir 追加 compute_workspace_hash；构造时不创建目录。new_flat 使用已隔离的 root。

证据：`crates/codegen/memory/src/storage.rs` — `new_inner`。

### Requirement: Search degradation
记忆检索 SHALL 在 embedding provider 缺失、失败或向量能力不可用时保留 FTS 检索路径。

#### Scenario: embedding 调用失败
- **WHEN** hybrid_search 无法取得查询向量
- **THEN** 以无查询向量进入合并阶段，仍返回符合配置的文本候选。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search`。

### Requirement: Bounded ranked search
记忆检索 SHALL 合并关键词与向量候选，并按来源、时间衰减、阈值及配置的结果数量输出。

#### Scenario: 检索候选过多
- **WHEN** 候选超过 max_results
- **THEN** 完成评分、过滤和可选 MMR 后限制返回数量；空模板不作为有用记忆返回。

证据：`crates/codegen/memory/src/search.rs` — `hybrid_search_merge`。
