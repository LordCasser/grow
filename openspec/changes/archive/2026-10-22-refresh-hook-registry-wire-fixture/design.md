# Design

核对既有 HookSpecWire、hooks::config::OnFailure 和 extension-runtime 契约；补样本 on_failure=allow，不给生产反序列化加默认值，也不移除失败断言。独立 HOME/GROW_HOME、固定 Rust 1.93.1 下先保留失败记录，再跑 workspace-types 全部库测试；仅提交本样本和本 change，类型布局工作独立验证归档。
