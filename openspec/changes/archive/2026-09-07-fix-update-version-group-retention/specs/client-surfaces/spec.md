## ADDED Requirements

### Requirement: Download retention groups platform artifacts by version
下载目录清理 SHALL 保留当前版本及当前版本之外最高版本的全部已识别平台产物。保留结果 SHALL 不依赖文件枚举顺序，其他候选仍受既有年龄保护。

#### Scenario: 多平台上一版本
- **WHEN** 当前版本与最高其他版本各有多个平台产物
- **THEN** 两个版本的全部产物均保留，明确过期的更旧版本产物可删除。

#### Scenario: 回退安装
- **WHEN** 安装版本低于下载目录中的其他版本
- **THEN** 仍保留当前版本及最高其他版本的全部平台产物，不将其他版本选择改成只允许低于当前版本。

证据入口：`crates/codegen/update/src/auto_update.rs` — `cleanup_old_downloads`。不是每个平台单独选择一个不同的保留版本。
