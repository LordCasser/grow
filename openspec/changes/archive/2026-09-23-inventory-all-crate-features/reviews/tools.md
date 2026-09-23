## tools crate 全量源码审计

完成 `crates/codegen/tools` 全量静态审计：Cargo.toml、build.rs、189个 Rust 源文件、5个集成测试、schema/notice 资源共198个文件，104960行源码；形成200条唯一功能需求，其中198条为本crate新增delta，2条既有跨crate契约补入tools来源。所有文件行数、SHA-256、测试计数和需求来源均已核对，未运行Cargo、构建脚本或外部下载。
