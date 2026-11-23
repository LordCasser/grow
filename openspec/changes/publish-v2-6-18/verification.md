## 本地候选

- 正式最新版为 v2.6.17。v2.6.18 标签在本地及远端均未占用。
- workspace 和锁文件的 33 个继承版本同步为 2.6.18。CLI 测试新增的两项依赖引用来自 MCP doctor change，外部包版本未漂移。
- Rust 1.93.1 下 Rustfmt、locked metadata、默认 CLI 检查、workspace all-targets 检查、`release-dist` 的 `-D warnings` 检查通过。all-targets 仍报告历史测试警告。
- 发布及测试执行脚本共 35 项，34 项通过，1 项按平台跳过。OpenSpec 严格校验 19 项、归档校验 986 项通过。暂存差异检查通过。
