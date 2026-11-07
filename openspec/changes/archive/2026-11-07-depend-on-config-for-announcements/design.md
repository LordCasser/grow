# Dependency boundary

父已读 announcements/Cargo.toml、src/lib.rs::announcements_state_path、tools/src/util/grow_home.rs及config/src/lib.rs根reexport。tools没有额外路径语义，只pub use config::grow_home，因此直接调用保同一OnceLock/env解析与路径行为。

Coder Task：只将announcements依赖从tools改config、唯一调用改config::grow_home，更新Cargo.lock中本地announcements依赖列表（不能改外部package版本/source/checksum或其它依赖），无需新测试。按Tier0核实际diff、定向fmt、cargo check/test该crate既有套件；Cargo窗口排在本轮实际ready行为修复之后。为合法local依赖边允许一次不带locked的offline命令刷新锁，再以结构化diff确认唯一预期条目，随后全部--locked --offline。保护已有Cargo.lock新增的memory依赖等dirty，不整体重写。父核完整diff/lock结构与实际检查后验收，并维护主docs/backlog/归档。
